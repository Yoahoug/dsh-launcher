#!/bin/bash
# 安装 dsh-launcher 开机自启(LaunchAgent):登录后起 launcher 服务,不开浏览器。
# 用法:scripts/install-launch-agent.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
LABEL="com.dshlauncher.agent"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
STATE_DIR="${DSH_LAUNCHER_STATE_DIR:-$HOME/.local/state/dsh-launcher}"
NODE_BIN="$(command -v node || echo /usr/local/bin/node)"

mkdir -p "$HOME/Library/LaunchAgents" "$STATE_DIR/logs"

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$NODE_BIN</string>
    <string>$ROOT/src/server.mjs</string>
  </array>
  <key>WorkingDirectory</key>
  <string>$ROOT</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StandardOutPath</key>
  <string>$STATE_DIR/logs/launchd.out.log</string>
  <key>StandardErrorPath</key>
  <string>$STATE_DIR/logs/launchd.err.log</string>
</dict>
</plist>
EOF

launchctl unload "$PLIST" >/dev/null 2>&1 || true
launchctl load -w "$PLIST"
echo "已安装开机自启:$LABEL(plist:$PLIST)"
