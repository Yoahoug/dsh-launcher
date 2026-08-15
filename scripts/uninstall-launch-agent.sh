#!/bin/bash
# 卸载 dsh-launcher 开机自启(LaunchAgent)。
# 用法:scripts/uninstall-launch-agent.sh
set -uo pipefail

LABEL="com.dshlauncher.agent"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"

launchctl unload "$PLIST" >/dev/null 2>&1 || true
rm -f "$PLIST"
echo "已卸载开机自启:$LABEL"
