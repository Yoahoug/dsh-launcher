# Windows CI:修复 `cargo test` 启动崩溃(0xc0000139)全过程

> 记录 v0.6.0 发布后修复 Windows CI `cargo test` 失败的完整过程:
> 现象 → 排查 → 根因 → 修复 → 验证,以及途中踩到的两个相关坑
> (macOS 打包 tar 带入 AppleDouble 文件、GitHub 受限时预置 NSIS 工具集)。

## 背景与现象

- 触发:推送 v0.6.0 后,`ci.yml`(macOS + Windows 矩阵)中 **Windows 的 `cargo test` 步骤失败**;
- 失败点:lib 单元测试二进制(`target/debug/deps/dsh_launcher_lib-*.exe`)以
  `0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)` 退出,进程根本无法启动;
- 关键事实:
  - macOS 的 `cargo test` 一直通过;
  - 集成测试二进制(`engine_flow`/`toolchain_install`)可以正常启动;
  - **只有 lib 的单元测试二进制**启动即崩;
  - 该失败是**历史遗留问题**:最早在 `fix: static-link MSVC runtime and embed Common
    Controls v6 manifest for Windows (build still failing, deferred)`(c65b54f)时已存在,
    codex 分支曾多轮尝试("ci: inspect Windows test imports"、"fix: embed Common Controls
    v6 in Windows tests" 等)后搁置。

## 排查过程

### 1. 定位失败步骤与差异

```
ci: macos-latest ✓ / windows-latest ✗(仅 cargo test 一步失败)
本地 Win 机器复现:lib 测试 exe 启动即 0xc0000139
对比:release app exe 正常启动(29MB 存活)、engine_flow 集成测试 exe 正常
→ 差异集中在「lib 单元测试二进制」
```

### 2. 用 dumpbin 分析导入表,找出缺失的入口点

```powershell
& dumpbin.exe /imports target\debug\deps\dsh_launcher_lib-*.exe
```

lib 测试 exe 的导入表出现 `comctl32.dll!TaskDialogIndirect`。
codex 当时的诊断脚本(逐函数解析)也给出同样结论:

```
Missing imported entry points:
  comctl32.dll!TaskDialogIndirect
```

### 3. 理解 TaskDialogIndirect 的版本陷阱

- `TaskDialogIndirect` 是 **Common Controls v6**(side-by-side 程序集
  `Microsoft.Windows.Common-Controls 6.0.0.0`)才导出的 API;
- 系统 `C:\Windows\System32\comctl32.dll` 是 v5,**没有该导出**;
- 只有 exe 内嵌了声明 v6 依赖的 **application manifest**,加载器才会绑定 WinSxS 里的 v6
  comctl32,`TaskDialogIndirect` 才能解析;
- 所以:谁缺 v6 清单,谁就 0xc0000139。

### 4. 用 mt.exe 验证各二进制的清单嵌入情况

```powershell
& mt.exe "-inputresource:<exe>;#1" -out:manifest.xml
```

| 二进制 | 内嵌 v6 清单? | 导入 TaskDialogIndirect? | 启动 |
|---|---|---|---|
| release app exe | ✅(来自 tauri 生成的 `resource.lib`) | ✅ | ✅ 正常 |
| engine_flow 集成测试 | ✅(来自本项目 build.rs) | ❌(该测试未引用 GUI 代码,链接期被裁掉) | ✅ 正常 |
| **lib 单元测试 exe** | ❌ **无任何资源段** | ✅ | ❌ 0xc0000139 |

### 5. 根因:build script 的链接参数作用域

项目 `src-tauri/build.rs` 原先用:

```rust
println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:\"{}\"", manifest.display());
```

用 `cargo build --tests -v` 抓取实际链接命令后发现:

- 集成测试(`tests/engine_flow.rs` 等)拿到了 `/MANIFEST:EMBED /MANIFESTINPUT:...` ✅;
- **lib 自身的 `--test` 目标(单元测试)没有拿到** —— `rustc-link-arg-tests`
  对 lib 单元测试目标不生效(它只作用于 `[[test]]` 集成测试目标);
- app bin 的清单来自 tauri-build(`WindowsAttributes` → `tauri_winres` 编译的
  `resource.lib`),与上述参数无关,所以 app 正常。

**结论:lib 单元测试二进制既没有 rustc-link-arg-tests 给的清单,也不链 resource.lib,
于是裸奔绑定 v5 comctl32 → 启动即崩。**

> 顺带发现:原写法里 `/MANIFESTINPUT:"{path}"` 带引号还会让 link.exe 把值当成
> 相对路径解析,拼出 `\\?\C:\C:\...` 报 `c1010070`(GNU 下链接失败)。路径不能加引号。

## 修复方案

改 `src-tauri/build.rs`,要点:

```rust
if target_os == windows {
    // 1) 全局嵌入清单:对 bin + 所有测试目标(含 lib 单元测试)统一生效
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display()); // 不带引号!
    // 2) 让 tauri 的 resource.lib 不再重复嵌清单(避免 bin 双重资源 → CVT1100/LNK1123)
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("failed to run tauri-build");
} else {
    tauri_build::build();
}
```

- 清单文件 `src-tauri/windows-test-manifest.xml` 内容与 tauri 默认 app 清单一致
  (仅 `Microsoft.Windows.Common-Controls 6.0.0.0` 依赖),因此 app 行为不变;
- 若保留 resource.lib 里的清单又加全局 `/MANIFEST:EMBED`,bin 会因重复资源报
  `CVTRES : fatal error CVT1100` + `LNK1123`,所以必须二选一(选全局,统一管理)。

## 验证

- **Windows 本机**(WSL 通道执行):
  - `cargo build --tests` ✅
  - `cargo build --release` ✅(release exe 重新生成)
  - `cargo test` ✅ —— **97 个 lib 测试全绿**(此前连启动都做不到);
    `engine_flow` 因 `#![cfg(unix)]` 在 Windows 上 0 tests 属预期;`toolchain_install`
    1 个网络测试 `#[ignore]` 属预期;
  - release exe 启动存活(27MB)、`taskkill` 清理 ✅;
- **macOS 本机**:`cargo fmt --check` ✅ / `cargo clippy --all-targets -- -D warnings` ✅
  / `cargo test`(97 + 5)✅;
- **GitHub Actions**:`ci.yml` 双端全绿 —— macOS 2m25s ✅、Windows 10m16s ✅
  (fmt、clippy、cargo test、`tauri build --debug --no-bundle` 全部通过)。

## 途中踩到的另外两个坑(Windows 侧构建环境)

### A. macOS 打包的 tar 带入 AppleDouble 文件

- 现象:Windows 侧 `cargo tauri build` 报
  `failed to read file 'capabilities\._default.json': stream did not contain valid UTF-8`;
- 根因:macOS bsdtar 默认会把 `com.apple.provenance` 等 xattr 写进 tar;GNU tar 1.35
  解包时把这些 xattr 物化成 **206 个 `._*` AppleDouble 文件**(散布全树),
  `._default.json` 被 tauri-build 当能力文件读取 → 非法 UTF-8 报错;
- 修复:打包时关闭 xattr 并排除 AppleDouble:

  ```bash
  COPYFILE_DISABLE=1 tar --no-xattrs --exclude='._*' --exclude='.git' \
    --exclude='node_modules' --exclude='src-tauri/target' --exclude='src-ui/dist' \
    -czf /tmp/src.tar.gz .
  ```

- 自查:`tar -tzf x.tar.gz | grep -c "\._"` 应为 0;git 只跟踪 `default.json`,仓库本身无 `._` 文件。

### B. 网络受限(GitHub 被墙)时预置 NSIS 工具集

- tauri 打包 NSIS 时会从 `github.com/tauri-apps/binary-releases` 下载 NSIS 3.11 +
  `nsis_tauri_utils.dll` 到 `%LOCALAPPDATA%\tauri\NSIS`(SHA1 校验,已存在且匹配则复用);
- 在无法访问 GitHub 的机器上,可在 Mac(可访问 GitHub)下载后转传预置:

  ```bash
  # 文件:nsis-3.11.zip / nsis_tauri_utils.dll(校验 SHA1)
  # 布局:%LOCALAPPDATA%\tauri\NSIS\(zip 解包后重命名 nsis-3.11 → NSIS)
  #       %LOCALAPPDATA%\tauri\NSIS\Plugins\x86-unicode\additional\nsis_tauri_utils.dll
  #       %LOCALAPPDATA%\tauri\MicrosoftEdgeWebview2Setup.exe(webview2 bootstrapper,可选)
  ```

- WebView2 bootstrapper(`go.microsoft.com` 可达时可直接让打包过程自行下载)。

## 关键结论 / 经验

1. **`cargo:rustc-link-arg-tests` 不覆盖 lib 单元测试目标**;需要"所有测试目标"时用
   **全局 `rustc-link-arg`**,并注意与既有清单来源(如 tauri 的 resource.lib)去重;
2. **`/MANIFESTINPUT:` 的值不要加引号**(link.exe 会把带引号值当相对路径);
3. **Windows GUI 相关依赖(tauri-plugin-dialog 等)会导入 `comctl32!TaskDialogIndirect`**,
   任何链接了这些依赖的二进制都必须有 Common Controls v6 清单,否则 0xc0000139;
4. Windows 加载期崩溃(`0xc0000139`/`0xc000007b`)优先查:内嵌清单(mt.exe)、
   导入表(dumpbin.exe /imports)、缺 v6 API;
5. macOS → Windows 传代码:用 `COPYFILE_DISABLE=1 tar --no-xattrs`,否则 GNU tar
   解出大量 `._*` 垃圾文件污染构建。

## 相关提交

- `8df5010` release: v0.6.0 — 首次运行引导修复,全屏顶部栏自动隐藏与左对齐
- `6a4e17b` style: cargo fmt
- `43c7102` style: fix clippy doc_lazy_continuation warning
- `ddb3a9a` fix: 统一嵌入 Common Controls v6 清单,修复 Windows cargo test 启动崩溃
