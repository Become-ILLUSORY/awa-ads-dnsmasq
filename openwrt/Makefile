include $(TOPDIR)/rules.mk

PKG_NAME:=awa-ads-dnsmasq
PKG_VERSION:=1.0.0
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_MAINTAINER:=Become-ILLUSORY

# 预编译二进制由 GitHub Actions 用 Rust 交叉编译产出，包由 OpenWrt SDK 生成。
#
# 这里刻意不设 PKG_SOURCE_URL：一旦设置，buildroot 会无视 dl/ 里已有的文件
# 去尝试下载，Release 不存在时就会失败。二进制由构建方预置到 dl/，
# 或用下面的 PREBUILT 变量指向本地路径。
PKG_BUILD_DEPENDS:=
PKG_BUILD_PARALLEL:=1

PKGARCH:=aarch64_cortex-a53

include $(INCLUDE_DIR)/package.mk

# 预编译二进制的位置：优先用包目录下的，其次 dl/
PREBUILT_BIN:=$(if $(wildcard $(CURDIR)/awa-ads-dnsmasq_linux_aarch64),$(CURDIR)/awa-ads-dnsmasq_linux_aarch64,$(DL_DIR)/awa-ads-dnsmasq_linux_aarch64)

define Build/Prepare
	mkdir -p $(PKG_BUILD_DIR)
	@if [ ! -f "$(PREBUILT_BIN)" ]; then \
		echo "错误: 找不到预编译二进制 $(PREBUILT_BIN)" >&2; \
		echo "      请把 awa-ads-dnsmasq_linux_aarch64 放到包目录或 dl/ 下" >&2; \
		exit 1; \
	fi
	$(CP) "$(PREBUILT_BIN)" $(PKG_BUILD_DIR)/awa-ads-dnsmasq
endef

define Package/$(PKG_NAME)
	SECTION:=utils
	CATEGORY:=Utilities
	TITLE:=DNS ad-blocking rule updater for dnsmasq
	URL:=https://github.com/Become-ILLUSORY/awa-ads-dnsmasq
	DEPENDS:=+dnsmasq-full
	PKGARCH:=$(PKGARCH)
endef

define Package/$(PKG_NAME)/description
 按 #Update time 判断秋风广告规则是否更新，改写为 NXDOMAIN 后重载 dnsmasq。
 静态链接，无运行时依赖。
endef

define Package/$(PKG_NAME)/conffiles
endef

define Build/Compile
endef

define Package/$(PKG_NAME)/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/awa-ads-dnsmasq $(1)/usr/bin/awa-ads-dnsmasq

	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/awa-ads-dnsmasq.init $(1)/etc/init.d/awa-ads-dnsmasq
endef

define Package/$(PKG_NAME)/postinst
#!/bin/sh
[ -n "$${IPKG_INSTROOT}" ] && exit 0
[ -x /etc/init.d/awa-ads-dnsmasq ] && /etc/init.d/awa-ads-dnsmasq enable
exit 0
endef

define Package/$(PKG_NAME)/prerm
#!/bin/sh
[ -n "$${IPKG_INSTROOT}" ] && exit 0
[ -x /etc/init.d/awa-ads-dnsmasq ] && /etc/init.d/awa-ads-dnsmasq stop
exit 0
endef

$(eval $(call BuildPackage,$(PKG_NAME)))
