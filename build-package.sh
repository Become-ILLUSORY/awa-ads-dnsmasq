#!/bin/sh
# 在 OpenWrt/ImmortalWrt 源码树或 SDK 里编译 awa-ads-dnsmasq 包。
# 24.10 出 .ipk，25.12 出 .apk，由 buildroot 自动决定。
#
# 用法: ./build-package.sh <openwrt源码或SDK目录> [版本号]

set -e

SRC=${1:?用法: $0 <openwrt目录> [版本号]}
VER=${2:-1.0.0}
HERE=$(cd "$(dirname "$0")" && pwd)

if [ ! -f "$SRC/rules.mk" ]; then
	echo "错误: $SRC 不像 OpenWrt 源码树或 SDK" >&2
	exit 1
fi

echo "==> 复制包到 $SRC/package/"
rm -rf "$SRC/package/awa-ads-dnsmasq"
cp -r "$HERE/openwrt" "$SRC/package/awa-ads-dnsmasq"

# 预编译二进制：优先用当前目录的，其次 dl/
mkdir -p "$SRC/dl"
if [ -f "$HERE/awa-ads-dnsmasq" ]; then
	echo "==> 放入预编译二进制"
	cp "$HERE/awa-ads-dnsmasq" "$SRC/dl/awa-ads-dnsmasq_linux_aarch64"
elif [ ! -f "$SRC/dl/awa-ads-dnsmasq_linux_aarch64" ]; then
	echo "错误: 找不到 awa-ads-dnsmasq 二进制" >&2
	echo "      请从 GitHub Releases 下载后放到 $HERE/awa-ads-dnsmasq" >&2
	exit 1
fi

cd "$SRC"
echo "==> 编译（版本 $VER）"
make package/awa-ads-dnsmasq/compile V=s

echo "==> 产物:"
find bin/packages -name "awa-ads-dnsmasq*" 2>/dev/null | head -5
