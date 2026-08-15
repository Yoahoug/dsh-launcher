// dsh-launcher · 测试沙箱:临时 HOME/CONFIG/STATE + fake git/pnpm/node 可执行文件
// 所有测试使用隔离目录和 fake 工具,绝不触碰真实 deepseek-harness 仓库。
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

let uid = 0

const FAKE_GIT = `#!/bin/bash
# fake git:行为由 FAKE_GIT_* 环境变量控制(测试注入),不访问真实远端
if [ "$1" = "--version" ]; then echo "git version 2.47.0"; exit 0; fi
case "$*" in
  "branch --show-current") echo "main"; exit 0 ;;
  "rev-parse --short HEAD") echo "abc1234"; exit 0 ;;
  *"rev-list --left-right --count HEAD...refs/remotes/origin/"*) echo "0 \${FAKE_GIT_BEHIND:-0}"; exit 0 ;;
  "status --porcelain") if [ -n "$FAKE_GIT_DIRTY" ]; then echo " M src/foo.js"; fi; exit 0 ;;
  "diff --name-only --diff-filter=U") if [ -n "$FAKE_GIT_CONFLICTS" ]; then echo "$FAKE_GIT_CONFLICTS"; fi; exit 0 ;;
  "fetch origin") if [ -n "$FAKE_GIT_FETCH_FAIL" ]; then echo "fatal: Could not resolve host: github.com" >&2; exit 1; fi; exit 0 ;;
  "stash push"*) if [ -n "$FAKE_GIT_STASH_FAIL" ]; then echo "error: cannot stash local changes" >&2; exit 1; fi; exit 0 ;;
  "pull --rebase --autostash")
    if [ -n "$FAKE_GIT_CONFLICT" ]; then
      mkdir -p "$FAKE_GIT_REPO/.git/rebase-merge"
      echo "main" > "$FAKE_GIT_REPO/.git/rebase-merge/head-name"
      echo "CONFLICT (content): Merge conflict in src/foo.js" >&2
      exit 1
    fi
    exit 0 ;;
esac
case "$*" in
  *"pnpm-lock.yaml"*) if [ -n "$FAKE_GIT_LOCKFILE" ]; then echo "pnpm-lock.yaml"; fi; exit 0 ;;
esac
echo "fake-git: unhandled args: $*" >&2
exit 1
`

const FAKE_PNPM = `#!/bin/bash
# fake pnpm:行为由 FAKE_PNPM_* 环境变量控制(测试注入)
if [ "$1" = "-v" ]; then echo "10.0.0"; exit 0; fi
case "$1" in
  dsh)
    sleep "\${FAKE_PNPM_READY_DELAY:-0.3}"
    if [ -z "$FAKE_PNPM_NO_READY" ]; then
      echo "dsh web: http://127.0.0.1:\${FAKE_PNPM_PORT:-9999}/"
    fi
    if [ -n "$FAKE_PNPM_EXIT_EARLY" ]; then
      echo "Error: boom" >&2
      exit 1
    fi
    while true; do sleep 1; done
    ;;
  run)
    if [ "$2" = "build" ]; then
      if [ -n "$FAKE_PNPM_BUILD_FAIL" ]; then
        echo "error TS1234: cannot find module 'x'" >&2
        exit 1
      fi
      echo "> build:lib:host"
      sleep 0.05
      echo "> build:lib:client"
      sleep 0.05
      echo "> build:web"
      exit 0
    fi
    if [ "$2" = "dev:web" ]; then
      echo "dev:web watcher ready"
      while true; do sleep 1; done
    fi
    echo "fake-pnpm: unknown run target $2" >&2; exit 1
    ;;
  install)
    if [ -n "$FAKE_PNPM_INSTALL_FAIL" ]; then echo "ERR_PNPM: install failed" >&2; exit 1; fi
    exit 0
    ;;
  *)
    echo "fake-pnpm: unhandled args: $*" >&2; exit 1
    ;;
esac
`

const FAKE_NODE = `#!/bin/bash
echo "\${FAKE_NODE_VERSION:-v24.19.0}"
`

function writeExecutable(dir, name, content) {
  const p = join(dir, name)
  writeFileSync(p, content, 'utf8')
  chmodSync(p, 0o755)
}

/** 创建隔离沙箱,返回 { root, configDir, stateDir, repoDir, binDir, cleanup }。 */
export function createSandbox() {
  const root = mkdtempSync(join(tmpdir(), `dsh-launcher-test-${process.pid}-${uid++}-`))
  const configDir = join(root, 'config')
  const stateDir = join(root, 'state')
  const repoDir = join(root, 'repo')
  const binDir = join(root, 'bin')
  for (const d of [configDir, stateDir, binDir]) mkdirSync(d)
  mkdirSync(join(repoDir, '.git'), { recursive: true })
  writeExecutable(binDir, 'git', FAKE_GIT)
  writeExecutable(binDir, 'pnpm', FAKE_PNPM)
  writeExecutable(binDir, 'node', FAKE_NODE)
  return {
    root,
    configDir,
    stateDir,
    repoDir,
    binDir,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/** 构造子进程环境:binDir 前置 PATH + 隔离的配置/状态目录 + fake 行为开关。 */
export function sandboxEnv(sb, extra = {}) {
  return {
    ...process.env,
    PATH: `${sb.binDir}:${process.env.PATH}`,
    DSH_LAUNCHER_CONFIG_DIR: sb.configDir,
    DSH_LAUNCHER_STATE_DIR: sb.stateDir,
    DSH_NO_AUTOOPEN: '1',
    FAKE_GIT_REPO: sb.repoDir,
    ...extra,
  }
}

/** 通用轮询:直到 pred() 真或超时。 */
export async function waitFor(pred, { timeout = 15000, interval = 100, label = 'condition' } = {}) {
  const t0 = Date.now()
  while (Date.now() - t0 < timeout) {
    if (await pred()) return
    await new Promise((r) => setTimeout(r, interval))
  }
  throw new Error(`超时等待 ${label}(${timeout}ms)`)
}
