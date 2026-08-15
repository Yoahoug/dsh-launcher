#!/bin/bash
# dsh-launcher · 双击启动入口(macOS .command)
# 行为:起服务(http://127.0.0.1:3090/)→ 自动打开浏览器亮色控制台;
# 已运行则只召回(单实例:端口探测 + pid 文件),绝不重复起服务。
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
HOST=127.0.0.1
PORT=3090
URL="http://$HOST:$PORT"
STATE_DIR="${DSH_LAUNCHER_STATE_DIR:-$HOME/.local/state/dsh-launcher}"
PIDFILE="$STATE_DIR/launcher.pid"
LOGFILE="$STATE_DIR/logs/launcher.out.log"

NODE_BIN="$(command -v node || true)"
[ -z "$NODE_BIN" ] && NODE_BIN="/usr/local/bin/node"

health() { curl -fsS -m 1 "$URL/api/health" >/dev/null 2>&1; }

# 打开控制台(脚本/测试场景可设 DSH_NO_AUTOOPEN=1,只打印 URL 不弹浏览器)
open_console() {
  if [ -n "${DSH_NO_AUTOOPEN:-}" ]; then
    echo "控制台就绪:$URL"
  else
    open "$URL"
  fi
}

# 1) 已在运行 → 召回控制台
if health; then
  open_console
  exit 0
fi

# 2) stale pid 兜底:进程活着但端口未就绪,再等一秒
if [ -f "$PIDFILE" ]; then
  OLD_PID="$(cat "$PIDFILE" 2>/dev/null || echo 0)"
  if kill -0 "$OLD_PID" 2>/dev/null; then
    sleep 1
    if health; then
      open_console
      exit 0
    fi
  fi
  rm -f "$PIDFILE"
fi

# 3) 起服务(后台常驻,日志落盘)
mkdir -p "$STATE_DIR/logs"
nohup "$NODE_BIN" "$ROOT/src/server.mjs" >> "$LOGFILE" 2>&1 &
echo $! > "$PIDFILE"

# 4) 等待就绪(<2s),就绪即打开控制台
for _ in $(seq 1 30); do
  if health; then
    open_console
    exit 0
  fi
  sleep 0.1
done

# 5) 失败提示(不闪退式退出,给出日志位置)
osascript -e "display alert \"dsh-launcher 启动失败\" message \"服务未能启动,请查看日志:$LOGFILE\"" >/dev/null 2>&1 || true
exit 1
