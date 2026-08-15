fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            // MSVC:全局嵌入 Common Controls v6 清单(与 tauri 默认 app 清单内容一致):
            // - app bin:resource.lib 不再重复嵌清单(见下方 new_without_app_manifest),统一由这里嵌入;
            // - 所有测试目标(含 lib 单元测试)也拿到清单 —— rustc-link-arg-tests 对 lib 的
            //   `--test` 目标不生效,而 lib 测试二进制导入了 comctl32!TaskDialogIndirect(v6 才有),
            //   没有清单会绑定系统 v5 comctl32,启动即 0xc0000139。
            // 路径不带引号:带引号会让 link.exe 把值当相对路径解析成 \\?\C:\C:\... 导致 c1010070。
            let manifest =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-test-manifest.xml");
            println!("cargo:rerun-if-changed={}", manifest.display());
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
            tauri_build::try_build(
                tauri_build::Attributes::new()
                    .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
            )
            .expect("failed to run tauri-build");
        } else {
            // GNU(交叉编译,macOS/Linux → Windows):/MANIFEST 系列是 MSVC link.exe 专有参数,
            // mingw 的 ld 不认识;这里让 tauri-build 用默认方式嵌入应用清单
            // (内部走 windres/embed-resource,兼容 mingw 链接器)。
            tauri_build::build();
        }
    } else {
        tauri_build::build();
    }
}
