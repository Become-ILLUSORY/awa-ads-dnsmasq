# awa-ads-dnsmasq

[![build](https://github.com/Become-ILLUSORY/awa-ads-dnsmasq/actions/workflows/build.yml/badge.svg)](https://github.com/Become-ILLUSORY/awa-ads-dnsmasq/actions/workflows/build.yml)

给 OpenWrt 的 dnsmasq 自动同步[秋风广告规则](https://github.com/TG-Twilight/AWAvenue-Ads-Rule)，拦截以 NXDOMAIN 返回。

Rust 编写，静态链接，单二进制 1.7 MB。只做一件事：判断上游是否更新，改写规则，写入 dnsmasq 并重载。

## 为什么用 NXDOMAIN

规则原文是 `address=/域名/#`，`#` 是 dnsmasq 的语法糖，返回 `0.0.0.0` 和 `::`。手册对此有明确警告：

> Note that NULL addresses normally work in the same way as localhost, so **beware that clients looking up these names are likely to end up talking to themselves.**

改写成 `server=/域名/` 后（手册说明等价于 `--address=/域名/`）：

- 对该域名及所有子域直接返回 NXDOMAIN，不转发、不泄漏查询
- 客户端立即失败，不会卡在连接超时
- 不会出现「客户端连自己」的怪异行为

## 工作方式

```
开机
  └─ /etc/init.d/awa-ads-dnsmasq
      └─ 等 DNS 能解析（最多 120 秒）
          └─ 从缓存恢复规则到 conf-dir，重启 dnsmasq

每天 04:00（cron）
  └─ awa-ads-dnsmasq
      ├─ 比对上游 #Update time
      ├─ 没变 → 直接退出，不重启
      └─ 变了 → 下载、校验、写入、重启 dnsmasq
```

**开机不依赖外网**：从 `/etc/awa-ads-dnsmasq/ads.conf` 缓存恢复，即使网络还没通也能立刻生效。

**唯一的中断点**：规则真的更新时重启 dnsmasq，DNS 中断 1-2 秒。通常一周发生 1-2 次，在凌晨。

> 曾尝试用 `servers-file` + SIGHUP 做免重启热重载。手册说该文件会在 SIGHUP 时重读，本地 `--no-daemon` 环境也验证有效，但在 OpenWrt 的 ujail 隔离环境下实测无效——信号发出去了，规则却没重载。用 `baidu.com` 做过严谨对照：`kill -HUP` 后仍返回真实 IP，`restart` 后才返回 NXDOMAIN。故放弃该方案。

## 安全设计

| 环节 | 保护 |
|---|---|
| 下载 | 11 个镜像按实测稳定性排序，每个源都校验版本号，防止镜像落后覆盖新规则 |
| 校验 | 写入前 `dnsmasq --test` 检查语法，不通过就放弃 |
| 应用 | 先写暂存文件再原子替换，dnsmasq 永远读到完整内容 |
| 暂存 | 临时文件以 `.` 开头，`conf-dir` 会跳过，残file 不会被加载 |
| 回滚 | 重启失败自动删除规则并重试，保证 DNS 至少恢复可用 |
| 断网 | 上游不可达时沿用本地缓存；无缓存则安全退出，不破坏现有配置 |

## 安装

```sh
# 1. 下载对应架构的二进制（目前只出 aarch64）
scp awa-ads-dnsmasq root@192.168.6.1:/usr/bin/awa-ads-dnsmasq
ssh root@192.168.6.1 chmod +x /usr/bin/awa-ads-dnsmasq

# 2. 干跑，确认能拿到规则
ssh root@192.168.6.1 /usr/bin/awa-ads-dnsmasq -n -v

# 3. 真跑一次
ssh root@192.168.6.1 /usr/bin/awa-ads-dnsmasq -v

# 4. 确认 DNS 正常
ssh root@192.168.6.1 nslookup baidu.com

# 5. 装 init 脚本，开机自动恢复
scp openwrt/files/awa-ads-dnsmasq.init root@192.168.6.1:/etc/init.d/awa-ads-dnsmasq
ssh root@192.168.6.1 'chmod +x /etc/init.d/awa-ads-dnsmasq && /etc/init.d/awa-ads-dnsmasq enable'
```

## 定时更新

```sh
ssh root@192.168.6.1 "
  echo '0 4 * * * /usr/bin/awa-ads-dnsmasq 2>&1 | logger -t awa-ads' >> /etc/crontabs/root
  /etc/init.d/cron enable
  /etc/init.d/cron restart
"
```

## 验证

```sh
# 拦截应生效（返回 NXDOMAIN 或空响应）
nslookup ad.doubleclick.net 127.0.0.1
nslookup a.market.xiaomi.com 127.0.0.1

# 正常域名不受影响
nslookup baidu.com 127.0.0.1
```

注意 `doubleclick.net` 本身不在规则里——秋风规则只拦具体子域（`ad.doubleclick.net`、`googleads.g.doubleclick.net`），因为该域下还有正常服务。

## 命令行

```
-s, --state <DIR>      缓存目录 [默认: /etc/awa-ads-dnsmasq]
-c, --confdir <DIR>    dnsmasq conf-dir，留空自动探测
-r, --restart <PATH>   dnsmasq 控制脚本 [默认: /etc/init.d/dnsmasq]
-n, --dry-run          只检查，不写入
-v, --verbose          输出详细信息
-V, --version          显示版本
-h, --help             显示帮助
```

## 文件位置

| 路径 | 内容 | 位置 |
|---|---|---|
| `/etc/awa-ads-dnsmasq/ads.conf` | 规则缓存 | flash，仅规则更新时写 |
| `/tmp/dnsmasq.<section>.d/awa-ads-dnsmasq.conf` | 生效的规则 | tmpfs，开机由 init 恢复 |
| `/etc/init.d/awa-ads-dnsmasq` | 开机恢复脚本 | flash |

conf-dir 路径由 OpenWrt 的 dnsmasq init 决定，24.10 起是 `/tmp/dnsmasq.<section>.d`，早期版本是 `/tmp/dnsmasq.d`。程序会通过 uci 自动探测。

## 卸载

```sh
/etc/init.d/awa-ads-dnsmasq disable    # 同时移除规则并重启 dnsmasq
rm -f /etc/init.d/awa-ads-dnsmasq
rm -rf /etc/awa-ads-dnsmasq
sed -i '/awa-ads-dnsmasq/d' /etc/crontabs/root
/etc/init.d/cron restart
```

## 局限

- 只拦域名，同域名下的正常内容会被一并拦截
- App 内置 DoH/DoT、写死 IP、QUIC 流量不受影响
- YouTube 广告与视频同域名，DNS 层无法区分
- 规则里有些域名属于推送/统计 SDK（如个推、友盟），拦截后部分国产 App 可能功能异常
- 仅出 aarch64 静态二进制，其他架构需自行编译

## 许可

规则文件版权归 [AWAvenue](https://github.com/TG-Twilight/AWAvenue-Ads-Rule)（GPLv3）。本项目仅做格式转换与分发，代码部分 MIT。
