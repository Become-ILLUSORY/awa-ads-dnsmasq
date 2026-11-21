//! 规则源与 HTTP 获取。

use std::io::{BufReader, Read};
use std::time::Duration;

use crate::rules::parse_update_time;

pub struct Source {
    pub name: &'static str,
    pub url: &'static str,
}

/// 实测稳定性排序：官方源优先，其次是能透传 Range 的 GitHub 代理，
/// 最后是可能缓存 12 小时的 jsDelivr。
pub const DEFAULT_SOURCES: &[Source] = &[
    Source { name: "github", url: "https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "ghfast", url: "https://ghfast.top/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "imciel", url: "https://ghproxy.imciel.com/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "ghproxy.net", url: "https://ghproxy.net/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "gh-proxy", url: "https://gh-proxy.com/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "gh-proxy.org", url: "https://gh-proxy.org/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "ghpr", url: "https://ghpr.cc/https://raw.githubusercontent.com/TG-Twilight/AWAvenue-Ads-Rule/main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "uura", url: "https://cdn.uura.cn/AWAvenue/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "cxplay", url: "https://script.cx.ms/awavenue/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "jsdelivr", url: "https://cdn.jsdelivr.net/gh/TG-Twilight/AWAvenue-Ads-Rule@main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
    Source { name: "fastly", url: "https://fastly.jsdelivr.net/gh/TG-Twilight/AWAvenue-Ads-Rule@main/Filters/AWAvenue-Ads-Rule-Dnsmasq.conf" },
];

const UA: &str = "awa-ads-dnsmasq/1.0 (+https://github.com/Become-ILLUSORY/awa-ads-dnsmasq)";

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(10))
        .user_agent(UA)
        .build()
}

/// 只取文件头一段内容解析版本号，避免整份下载。
/// 部分代理忽略 Range 会返回整个文件，此时读够头部就丢弃剩余部分。
pub fn probe_timestamp(agent: &ureq::Agent, src: &Source) -> Result<String, String> {
    let resp = agent
        .get(src.url)
        .set("Range", "bytes=0-8191")
        .call()
        .map_err(|e| format!("{}: {e}", src.name))?;

    let mut reader = BufReader::with_capacity(8192, resp.into_reader());
    parse_update_time(&mut reader).map_err(|e| format!("{}: {e}", src.name))
}

/// 打开规则流，调用方负责读取。
pub fn open_rules(agent: &ureq::Agent, src: &Source) -> Result<impl Read, String> {
    let resp = agent
        .get(src.url)
        .call()
        .map_err(|e| format!("{}: {e}", src.name))?;
    Ok(resp.into_reader())
}

/// 探测各源，返回可得的最高版本。
/// 镜像可能缓存旧版本，拿到落后于本地的结果时要继续试下一个源。
pub fn probe_best(
    agent: &ureq::Agent,
    local_ts: &str,
) -> Result<(String, &'static Source), String> {
    let mut best: Option<(String, &'static Source)> = None;
    let mut last_err = String::new();

    for src in DEFAULT_SOURCES {
        match probe_timestamp(agent, src) {
            Ok(ts) => {
                let newer = best.as_ref().is_none_or(|(b, _)| ts > *b);
                if newer {
                    best = Some((ts.clone(), src));
                }
                // 已拿到不低于本地的版本，无需再探测
                if local_ts.is_empty() || ts.as_str() >= local_ts {
                    return Ok((ts, src));
                }
            }
            Err(e) => last_err = e,
        }
    }

    best.ok_or_else(|| {
        if last_err.is_empty() {
            "所有规则源均不可用".to_string()
        } else {
            last_err
        }
    })
}
