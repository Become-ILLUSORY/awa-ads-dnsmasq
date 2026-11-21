//! 给 OpenWrt 的 dnsmasq 同步秋风广告规则。
//!
//! 只做一件事：判断上游是否更新，改写为 NXDOMAIN，写入 confdir 并重启 dnsmasq。
//! 任何一步失败都不会破坏现有 DNS 配置。

mod app;
mod fetch;
mod rules;

use std::path::PathBuf;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() {
    eprintln!("awa-ads-dnsmasq {VERSION}");
    eprintln!();
    eprintln!("同步秋风广告规则到 dnsmasq，拦截以 NXDOMAIN 返回。");
    eprintln!();
    eprintln!("用法: awa-ads-dnsmasq [选项]");
    eprintln!();
    eprintln!("  -s, --state <DIR>      缓存目录 [默认: /etc/awa-ads-dnsmasq]");
    eprintln!("  -c, --confdir <DIR>    dnsmasq conf-dir，留空自动探测");
    eprintln!("  -r, --restart <PATH>   dnsmasq 控制脚本 [默认: /etc/init.d/dnsmasq]");
    eprintln!("  -n, --dry-run          只检查，不写入");
    eprintln!("  -v, --verbose          输出详细信息");
    eprintln!("  -V, --version          显示版本");
    eprintln!("  -h, --help             显示帮助");
}

fn main() -> ExitCode {
    let mut opts = app::Options::default();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-s" | "--state" => match args.next() {
                Some(v) => opts.state_dir = PathBuf::from(v),
                None => {
                    eprintln!("缺少 --state 参数值");
                    return ExitCode::from(2);
                }
            },
            "-c" | "--confdir" => match args.next() {
                Some(v) => opts.conf_dir = Some(PathBuf::from(v)),
                None => {
                    eprintln!("缺少 --confdir 参数值");
                    return ExitCode::from(2);
                }
            },
            "-r" | "--restart" => match args.next() {
                Some(v) => opts.restart = PathBuf::from(v),
                None => {
                    eprintln!("缺少 --restart 参数值");
                    return ExitCode::from(2);
                }
            },
            "-n" | "--dry-run" => opts.dry_run = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-V" | "--version" => {
                println!("{VERSION}");
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("未知参数: {other}");
                usage();
                return ExitCode::from(2);
            }
        }
    }

    match app::run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误: {e}");
            ExitCode::FAILURE
        }
    }
}
