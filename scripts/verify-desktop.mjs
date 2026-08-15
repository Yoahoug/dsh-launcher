#!/usr/bin/env node
// 跨平台一键门禁：串行执行全部桌面验证步骤。
// 用途：避免 Node integration 与 Cargo E2E 并行争用 3090；
// M4 移除 3090 后依然保持串行，保证可重复。
import { spawnSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauriDir = join(root, 'src-tauri');
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));

const hasScript = (name) => Boolean(pkg.scripts?.[name]);

const steps = [
  { name: 'pnpm check', run: 'pnpm', args: ['check'], onlyIf: () => hasScript('check') },
  { name: 'pnpm test', run: 'pnpm', args: ['test'], onlyIf: () => hasScript('test') },
  { name: 'pnpm test:ui', run: 'pnpm', args: ['test:ui'], onlyIf: () => hasScript('test:ui') },
  { name: 'pnpm typecheck', run: 'pnpm', args: ['typecheck'] },
  { name: 'pnpm build:renderer', run: 'pnpm', args: ['build:renderer'] },
  { name: 'cargo fmt --check', run: 'cargo', args: ['fmt', '--check'], cwd: tauriDir },
  { name: 'cargo clippy', run: 'cargo', args: ['clippy', '--all-targets', '--', '-D', 'warnings'], cwd: tauriDir },
  { name: 'cargo test', run: 'cargo', args: ['test'], cwd: tauriDir },
  { name: 'tauri build --debug --no-bundle', run: 'pnpm', args: ['tauri', 'build', '--debug', '--no-bundle'] },
];

let failed = false;
for (const step of steps) {
  if (step.onlyIf && !step.onlyIf()) {
    console.log(`[skip] ${step.name} (脚本不存在)`);
    continue;
  }
  console.log(`[run] ${step.name}`);
  const res = spawnSync(step.run, step.args, {
    cwd: step.cwd ?? root,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: { ...process.env, PATH: `${process.env.HOME}/.cargo/bin:${process.env.PATH ?? ''}` },
  });
  if (res.status !== 0) {
    console.error(`[FAIL] ${step.name} (exit ${res.status})`);
    failed = true;
    break;
  }
  console.log(`[ok] ${step.name}`);
}

if (failed) {
  console.error('\nverify-desktop: 存在失败步骤，已停止。');
  process.exit(1);
}
console.log('\nverify-desktop: 全部门禁通过。');
