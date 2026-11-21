# awa-ads-dnsmasq

给 OpenWrt 的 dnsmasq 自动同步 [秋风广告规则](https://github.com/TG-Twilight/AWAvenue-Ads-Rule)，用 NXDOMAIN 拦截广告域名。

Rust 编写，静态链接，单二进制，1.7 MB。

## 核心设计：热重载，不重启 DNS

dnsmasq 手册明确：

> **SIGHUP does NOT re-read the configuration file.**

也就是说，改 `conf-dir` 里的文件后发 SIGHUP 是无效的，必须重启 dnsmasq——重启期间所有 DNS 查询失败，会造成短暂断网。

但手册还有一条：

> `--servers-file=<file>` — the file is re-read and the configuration therein is **updated when dnsmasq receives SIGHUP**.

而 `--address=/example.com/` 与 `--server=/example.com/` 语义完全相同，都返回 NXDOMAIN：

> one or more domains with no address returns a no-such-domain answer, so `--address=/example.com/` is equivalent to `--server=/example.com/`

所以本工具把规则写成 `server=/域名/` 放进 servers-file，更新后发 SIGHUP 热重载，**DNS 全程不中断**。

## 为什么用 NXDOMAIN

规则原文是 `address=/域名/#`，`#` 是 dnsmasq 的语法糖，返回 `0.0.0.0` 和 `::`。手册对此有警告：

> Note that NULL addresses normally work in the same way as localhost, so **beware that clients looking up these names are likely to end up talking to themselves.**

改写成 `server=/域名/` 后：

- 对所有记录类型直接返回 NXDOMAIN，不转发、不泄漏
- 客户端立即失败，不会卡在连接超时
- 不会出现「客户端连自己」的怪异行为

## 安全设计

| 环节 | 保护 |
|---|---|
| 下载 | 多源回退，每个源校验版本号，防止镜像落后覆盖新规则 |
| 校验 | 写入前 `dnsmasq --test` 检查语法，不通过就放弃 |
| 应用 | 先写临时文件再原子替换，dnsmasq 永远读到完整内容 |
| 暂存 | 临时文件以 `.` 开头，`conf-dir` 会跳过，残file 不会被加载 |
| 断网 | 上游不可达时沿用本地缓存；无缓存则安全退出 |

## 安装

```sh
# 下载对应架构的二进制（目前只出 aarch64）
scp awa-ads-dnsmasq root@192.168.1.1:/usr/bin/awa-ads-dnsmasq
ssh root@192.168.1.1 chmod +x /usr/bin/awa-ads-dnsmasq

# 先干跑，确认能拿到规则
ssh root@192.168.1.1 /usr/bin/awa-ads-dnsmasq -n -v

# 真跑一次
ssh root@192.168.1.1 /usr/bin/awa-ads-dnsmasq -v

# 确认 DNS 正常
ssh root@192.168.1.1 nslookup baidu.com
```

首次运行会写 `conf-dir` 并重启一次 dnsmasq。要让后续更新免重启，配置 servers-file：

```sh
mkdir -p /etc/awa-ads-dnsmasq && touch /etc/awa-ads-dnsmasq/servers.conf
uci set dhcp.@dnsmasq[0].serversfile='/etc/awa-ads-dnsmasq/servers.conf'
uci commit dhcp
/etc/init.d/dnsmasq restart
```

之后每次更新只发 SIGHUP，DNS 不中断。

## 定时更新

```sh
echo '0 4 * * * /usr/bin/awa-ads-dnsmasq 2>&1 | logger -t awa-ads-dnsmasq' >> /etc/crontabs/root
/etc/init.d/cron enable
/etc/init.d/cron restart
```

## 命令行

```
-s, --state <DIR>      缓存目录 [默认: /etc/awa-ads-dnsmasq]
-c, --confdir <DIR>    dnsmasq conf-dir，留空自动探测
-r, --restart <PATH>   dnsmasq 控制脚本 [默认: /etc/init.d/dnsmasq]
-n, --dry-run          只检查，不写入
-v, --verbose          输出详细信息
-V, --version          显示版本
```

## 文件位置

| 路径 | 内容 |
|---|---|
| `/etc/awa-ads-dnsmasq/ads.conf` | 规则缓存（`server=/域名/` 格式） |
| `/etc/awa-ads-dnsmasq/servers.conf` | 生效的规则（配了 servers-file 时） |
| `/tmp/dnsmasq.d/awa-ads-dnsmasq.conf` | 生效的规则（未配 servers-file 时） |

## 卸载

```sh
/etc/init.d/awa-ads-dnsmasq disable
rm -rf /etc/awa-ads-dnsmasq
rm -f /tmp/dnsmasq*.d/awa-ads-dnsmasq.conf
uci delete dhcp.@dnsmasq[0].serversfile
uci commit dhcp
/etc/init.d/dnsmasq restart
```

## 局限

- 只拦域名，同域名下的正常内容会被一并拦截
- App 内置 DoH/DoT、写死 IP、QUIC 流量不受影响
- YouTube 广告与视频同域名，DNS 层无法区分
- 规则里有些域名属于推送/统计 SDK（如个推、友盟），拦截后部分国产 App 可能功能异常

## 许可

规则文件版权归 [AWAvenue](https://github.com/TG-Twilight/AWAvenue-Ads-Rule)（GPLv3）。本项目仅做格式转换与分发，代码部分 MIT。
