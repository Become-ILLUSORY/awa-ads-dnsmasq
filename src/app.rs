//! 规则更新：下载、改写、校验、应用。
//!
//! 只做一件事，任何一步失败都不碰现有配置。

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fetch::{self, Source};
use crate::rules;

pub struct Options {
    pub state_dir: PathBuf,
    pub conf_dir: Option<PathBuf>,
    pub restart: PathBuf,
    pub dry_run: bool,
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            state_dir: PathBuf::from("/etc/awa-ads-dnsmasq"),
            conf_dir: None,
            restart: PathBuf::from("/etc/init.d/dnsmasq"),
            dry_run: false,
            verbose: false,
        }
    }
}

fn log(msg: &str) {
    println!("{msg}");
}

pub fn run(opts: &Options) -> Result<(), String> {
    fs::create_dir_all(&opts.state_dir)
        .map_err(|e| format!("创建状态目录 {}: {e}", opts.state_dir.display()))?;

    let cache = opts.state_dir.join("ads.conf");
    let local_ts = read_header_time(&cache).unwrap_or_default();
    if !local_ts.is_empty() && opts.verbose {
        log(&format!("本地规则版本: {local_ts}"));
    }

    let agent = fetch::agent();
    let (remote_ts, src) = match fetch::probe_best(&agent, &local_ts) {
        Ok(v) => v,
        Err(e) => {
            if local_ts.is_empty() {
                return Err(format!("获取上游版本失败且无本地缓存: {e}"));
            }
            log(&format!("上游不可达({e})，沿用本地规则 {local_ts}"));
            return apply(opts, &cache);
        }
    };

    if remote_ts == local_ts {
        log(&format!("已是最新: {remote_ts}"));
        return apply(opts, &cache);
    }

    // 镜像可能缓存旧版本，不能用它覆盖更新的本地规则
    if !local_ts.is_empty() && remote_ts < local_ts {
        log(&format!(
            "上游版本({remote_ts})旧于本地({local_ts})，跳过更新"
        ));
        return apply(opts, &cache);
    }

    let old = if local_ts.is_empty() {
        "无"
    } else {
        local_ts.as_str()
    };
    log(&format!("发现新版本: {old} -> {remote_ts}"));

    if opts.dry_run {
        log(&format!(
            "dry-run: 将下载 {} 并写入 {}",
            src.name,
            cache.display()
        ));
        return Ok(());
    }

    match download(&agent, opts, src, &remote_ts, &cache) {
        Ok(n) => log(&format!("已更新: {remote_ts} ({n} 条规则)")),
        Err(e) => {
            if local_ts.is_empty() {
                return Err(e);
            }
            log(&format!("下载失败({e})，保留现有规则"));
        }
    }

    apply(opts, &cache)
}

fn read_header_time(path: &Path) -> Option<String> {
    let f = File::open(path).ok()?;
    let mut r = BufReader::new(f);
    rules::parse_update_time(&mut r).ok()
}

/// 下载并改写，成功后原子替换缓存。
fn download(
    agent: &ureq::Agent,
    opts: &Options,
    primary: &'static Source,
    want_ts: &str,
    cache: &Path,
) -> Result<usize, String> {
    let mut ordered: Vec<&'static Source> = vec![primary];
    ordered.extend(
        fetch::DEFAULT_SOURCES
            .iter()
            .filter(|s| s.url != primary.url),
    );

    let mut last_err = String::new();
    for src in ordered {
        match download_one(agent, opts, src, want_ts, cache) {
            Ok(n) => return Ok(n),
            Err(e) => {
                if opts.verbose {
                    log(&format!("源 {} 失败: {e}", src.name));
                }
                last_err = e;
            }
        }
    }
    Err(last_err)
}

fn download_one(
    agent: &ureq::Agent,
    opts: &Options,
    src: &Source,
    want_ts: &str,
    cache: &Path,
) -> Result<usize, String> {
    let stream = fetch::open_rules(agent, src)?;
    let mut br = BufReader::with_capacity(64 * 1024, stream);

    let got_ts = rules::parse_update_time(&mut br).map_err(|e| e.to_string())?;
    if got_ts != want_ts {
        return Err(format!("内容版本 {got_ts} 与探测结果 {want_ts} 不一致"));
    }

    let tmp = opts.state_dir.join(".ads.tmp");
    let result = (|| -> Result<usize, String> {
        let f = File::create(&tmp).map_err(|e| format!("创建临时文件: {e}"))?;
        let mut bw = BufWriter::with_capacity(32 * 1024, f);

        // parse_update_time 已消耗掉头部，这里补回版本行，
        // 否则下次运行读不到版本号，会误判为「无本地版本」而重复下载。
        bw.write_all(format!("#Update time: {got_ts}\n").as_bytes())
            .map_err(|e| e.to_string())?;

        let n = rules::rewrite(&mut br, &mut bw).map_err(|e| e.to_string())?;
        if n < rules::MIN_RULE_COUNT {
            return Err(format!("规则数量异常({n})，拒绝应用"));
        }
        bw.flush().map_err(|e| e.to_string())?;
        let f = bw.into_inner().map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);

        fs::rename(&tmp, cache).map_err(|e| format!("替换缓存: {e}"))?;
        Ok(n)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// 把缓存写入 dnsmasq confdir，校验后重启。失败自动回滚。
fn apply(opts: &Options, cache: &Path) -> Result<(), String> {
    if !cache.exists() {
        log("无可用规则，跳过");
        return Ok(());
    }

    let dir = opts.conf_dir.clone().unwrap_or_else(detect_conf_dir);
    let target = dir.join("awa-ads-dnsmasq.conf");

    // 配了 servers-file 就以它为准：规则可能还没同步过去，
    // 只检查 conf-dir 会漏掉这种情况。
    let sf = servers_file();
    let effective = match &sf {
        Some(p) => p.clone(),
        None => target.clone(),
    };

    if same_content(cache, &effective) {
        if opts.verbose {
            log(&format!("规则已是当前版本: {}", effective.display()));
        }
        return Ok(());
    }
    if opts.dry_run {
        log(&format!("dry-run: 将写入 {}", effective.display()));
        return Ok(());
    }

    fs::create_dir_all(&dir).map_err(|e| format!("创建 confdir {}: {e}", dir.display()))?;

    // 先写暂存文件并校验语法。暂存名以点开头，conf-dir 会跳过它，
    // 万一校验失败留下残file也不会被 dnsmasq 读到。
    let staged = dir.join(".awa-ads-dnsmasq.conf.tmp");
    fs::copy(cache, &staged).map_err(|e| format!("写入暂存文件: {e}"))?;

    match validate(&staged) {
        Ok(true) => {}
        Ok(false) => log("dnsmasq 不可执行，跳过语法校验"),
        Err(e) => {
            let _ = fs::remove_file(&staged);
            return Err(format!("规则语法校验失败，已放弃: {e}"));
        }
    }

    // 配了 servers-file 就写那里并发 SIGHUP：dnsmasq 会热重载该文件，
    // DNS 全程不中断。没配则退回 conf-dir + 重启。
    if let Some(sf) = &sf {
        if let Err(e) = write_servers_file(sf, &staged) {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
        let _ = fs::remove_file(&staged);

        // 清掉 conf-dir 里的旧文件：配了 servers-file 后它不再更新，
        // 留着会让上游已删除的域名一直被拦。删掉后 dnsmasq 只认 servers-file。
        if target.exists() {
            let _ = fs::remove_file(&target);
            log(&format!("已移除旧的 {}", target.display()));
        }

        log(&format!(
            "已更新 {}，SIGHUP 热重载（DNS 不中断）",
            sf.display()
        ));
        return sighup_dnsmasq();
    }

    if let Err(e) = fs::rename(&staged, &target) {
        let _ = fs::remove_file(&staged);
        return Err(format!("替换规则文件: {e}"));
    }
    log(&format!(
        "已写入 {}，重启 dnsmasq（配置 servers-file 可免重启）",
        target.display()
    ));

    match restart(opts) {
        Ok(()) => Ok(()),
        Err(e) => {
            log(&format!("重启失败({e})，回滚"));
            let _ = fs::remove_file(&target);
            match restart(opts) {
                Ok(()) => Err(format!("新规则导致 dnsmasq 无法启动，已回滚: {e}")),
                Err(e2) => Err(format!("回滚后仍无法启动，请手动检查: {e2}")),
            }
        }
    }
}

/// 读 uci 配置里的 serversfile。OpenWrt 的 dnsmasq init 支持这个选项。
fn servers_file() -> Option<PathBuf> {
    let out = Command::new("uci")
        .args(["-q", "get", "dhcp.@dnsmasq[0].serversfile"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// 原子写入 servers-file，保证 dnsmasq 读到的永远是完整内容。
fn write_servers_file(sf: &Path, staged: &Path) -> Result<(), String> {
    if let Some(dir) = sf.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 {}: {e}", dir.display()))?;
    }
    let tmp = sf.with_extension("tmp");
    fs::copy(staged, &tmp).map_err(|e| format!("写入 {}: {e}", tmp.display()))?;
    fs::rename(&tmp, sf).map_err(|e| format!("替换 {}: {e}", sf.display()))?;
    Ok(())
}

/// 发 SIGHUP 让 dnsmasq 重读 servers-file。
/// 手册明确：SIGHUP 不重读配置文件，但会重读 servers-file。
fn sighup_dnsmasq() -> Result<(), String> {
    let out = Command::new("killall")
        .args(["-HUP", "dnsmasq"])
        .output()
        .map_err(|e| format!("执行 killall: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "发送 SIGHUP 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// dnsmasq --test 校验。返回 Ok(false) 表示无法校验。
fn validate(path: &Path) -> Result<bool, String> {
    let out = match Command::new("dnsmasq")
        .arg("--test")
        .arg("-C")
        .arg(path)
        .output()
    {
        Ok(o) => o,
        Err(_) => return Ok(false),
    };
    if out.status.success() {
        return Ok(true);
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

fn restart(opts: &Options) -> Result<(), String> {
    let out = Command::new(&opts.restart)
        .arg("restart")
        .output()
        .map_err(|e| format!("执行 {}: {e}", opts.restart.display()))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn same_content(a: &Path, b: &Path) -> bool {
    let (ma, mb) = match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => (x, y),
        _ => return false,
    };
    if ma.len() != mb.len() {
        return false;
    }
    match (fs::read(a), fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// dnsmasq 的 conf-dir。24.10 起默认 /tmp/dnsmasq.<section>.d。
pub fn detect_conf_dir() -> PathBuf {
    if let Ok(out) = Command::new("uci")
        .args(["-q", "get", "dhcp.@dnsmasq[0].confdir"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let s = s.trim();
        if !s.is_empty() {
            if let Some(first) = s.split(',').next() {
                return PathBuf::from(first);
            }
        }
    }

    if let Ok(entries) = fs::read_dir("/tmp") {
        let mut found: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("dnsmasq") && n.ends_with(".d"))
            })
            .collect();
        found.sort();
        if let Some(p) = found.into_iter().next() {
            return p;
        }
    }

    PathBuf::from("/tmp/dnsmasq.d")
}
