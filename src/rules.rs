//! 规则文件的解析与改写。

use std::io::{BufRead, Write};

pub const UPDATE_TIME_PREFIX: &str = "#Update time:";
pub const ADDRESS_PREFIX: &str = "address=/";
pub const SERVER_PREFIX: &str = "server=/";
/// 头部最多扫描这么多行，避免无头文件被整份读完
const HEADER_MAX_LINES: usize = 50;
/// 少于这个条数视为异常响应，拒绝应用
pub const MIN_RULE_COUNT: usize = 100;

#[derive(Debug)]
pub enum RuleError {
    NoTimestamp,
    BadTimestamp,
    Io(std::io::Error),
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTimestamp => write!(f, "规则文件缺少 #Update time 头"),
            Self::BadTimestamp => write!(f, "规则文件 #Update time 格式异常"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RuleError {}

impl From<std::io::Error> for RuleError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// 从 BufRead 中逐行读取头部，找到 `#Update time: ...` 即返回。
/// 只消耗头部那几行，其余内容仍留在 reader 里，因此不会因预读丢数据。
pub fn parse_update_time<R: BufRead>(r: &mut R) -> Result<String, RuleError> {
    let mut line = String::with_capacity(256);

    for _ in 0..HEADER_MAX_LINES {
        line.clear();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            return Err(RuleError::NoTimestamp);
        }

        let trimmed = line.trim();
        if let Some(v) = trimmed.strip_prefix(UPDATE_TIME_PREFIX) {
            let v = v.trim();
            if !is_timestamp(v) {
                return Err(RuleError::BadTimestamp);
            }
            return Ok(v.to_string());
        }
    }
    Err(RuleError::NoTimestamp)
}

/// 校验形如 `2026-11-16 17:36:05 UTC+8` 的前半段。
fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 19 {
        return false;
    }
    b[..19].iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        10 => *c == b' ',
        13 | 16 => *c == b':',
        _ => c.is_ascii_digit(),
    })
}

/// 把 `address=/域名/#` 改写为 `address=/域名/`，
/// 让 dnsmasq 对该域名所有记录类型直接返回 NXDOMAIN，而不是 0.0.0.0。
/// 逐行流式处理，内存占用与文件大小无关。返回写出的规则条数。
/// 把 `address=/域名/#` 改写为 `server=/域名/`。
///
/// 手册说 `--address=/example.com/`、`--server=/example.com/`、
/// `--local=/example.com/` 三者等价，都返回 NXDOMAIN。已在
/// dnsmasq 2.90（ImmortalWrt 24.10）与 2.93 上实测确认。
///
/// 选 `server=` 是因为 servers-file 只接受 server/rev-server，
/// 而 servers-file 会在 SIGHUP 时热重载——更新规则不用重启
/// dnsmasq，DNS 全程不中断。
pub fn rewrite<R: BufRead, W: Write>(r: &mut R, w: &mut W) -> Result<usize, RuleError> {
    let mut count = 0;
    let mut line = String::with_capacity(512);

    loop {
        line.clear();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            break;
        }

        let s = line.trim_end_matches(['\r', '\n', ' ', '\t']);

        // 只处理 address 行，注释和空行原样保留（servers-file 允许注释）
        let out = if let Some(rest) = s.strip_prefix(ADDRESS_PREFIX) {
            count += 1;
            // address=/域名/#  ->  server=/域名/
            let domain = rest.strip_suffix('#').unwrap_or(rest);
            format!("{SERVER_PREFIX}{domain}")
        } else {
            s.to_string()
        };

        w.write_all(out.as_bytes())?;
        w.write_all(b"\n")?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const HEADER: &str = "#Title: AWAvenue Ads Rule\n\
#Total lines: 978\n\
#Update time: 2026-11-16 17:36:05 UTC+8\n\
\n\
address=/1rtb.com/#\n\
address=/a.market.xiaomi.com/#\n";

    #[test]
    fn parses_timestamp() {
        let mut c = Cursor::new(HEADER);
        assert_eq!(
            parse_update_time(&mut c).unwrap(),
            "2026-11-16 17:36:05 UTC+8"
        );
    }

    #[test]
    fn header_parse_keeps_body() {
        let mut c = Cursor::new(HEADER);
        parse_update_time(&mut c).unwrap();

        let mut out = Vec::new();
        let n = rewrite(&mut c, &mut out).unwrap();
        assert_eq!(n, 2, "body lost after header parse");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("server=/1rtb.com/"));
    }

    #[test]
    fn missing_timestamp() {
        let mut c = Cursor::new("#Title: x\naddress=/a/#\n");
        assert!(matches!(
            parse_update_time(&mut c),
            Err(RuleError::NoTimestamp)
        ));
    }

    #[test]
    fn malformed_timestamp() {
        let mut c = Cursor::new("#Update time: not-a-date\n");
        assert!(matches!(
            parse_update_time(&mut c),
            Err(RuleError::BadTimestamp)
        ));
    }

    #[test]
    fn rewrite_strips_hash() {
        let input = "#Title: x\n\
address=/1rtb.com/#\n\
address=/already.local/\n\
# comment\n";
        let mut out = Vec::new();
        let n = rewrite(&mut Cursor::new(input), &mut out).unwrap();

        assert_eq!(n, 2);
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("address="), "不应残留 address 指令");
        assert!(!s.contains("/#"));
        assert!(s.contains("server=/1rtb.com/\n"));
        assert!(s.contains("server=/already.local/\n"));
        assert!(s.contains("# comment\n"));
    }

    #[test]
    fn rewrite_no_trailing_newline() {
        let mut out = Vec::new();
        let n = rewrite(&mut Cursor::new("address=/last.example/#"), &mut out).unwrap();
        assert_eq!(n, 1);
        assert_eq!(String::from_utf8(out).unwrap(), "server=/last.example/\n");
    }
}
