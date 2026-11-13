#!/bin/bash
# Seal a Mac app with the configured local identity, or ad-hoc on an unconfigured machine.
set -euo pipefail

app="$1"
identity="${PONI_MAC_SIGN_IDENTITY:-}"
config="$HOME/Library/Application Support/Island For Mac/signing-identity.txt"

if [ -z "$identity" ] && [ -f "$config" ]; then
  identity="$(cat "$config")"
  # 配置只存证书指纹；失效时让签名失败，不能静默退回临时身份。
  if ! [[ "$identity" =~ ^[0-9A-Fa-f]{40}$ ]]; then
    echo "Invalid local signing identity: $config" >&2
    exit 1
  fi
fi

if [ -z "$identity" ]; then
  identity="-"
fi

# 本地签名不访问时间戳服务器；正式 Developer ID 公证仍由 sign_mac_dmg.sh 负责。
codesign --force --deep --timestamp=none --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
