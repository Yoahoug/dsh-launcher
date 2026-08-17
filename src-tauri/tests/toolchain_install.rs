// dsh-launcher · 真实托管工具链安装集成测试(需网络,默认忽略)
//
// 运行:cargo test --test toolchain_install -- --ignored --nocapture
// 覆盖:签名 catalog → 国内镜像下载 → 长度/SHA-256 校验 → 安全解压 → 自检 → active pointer。
// 只安装「缺失」的组件(幂等);使用隔离 DSH_LAUNCHER_STATE_DIR,不触碰真实数据。
use dsh_launcher_lib::log_hub::LogHub;
use dsh_launcher_lib::ops::CancellationToken;
use dsh_launcher_lib::services::runtime::{self, Tools};
use dsh_launcher_lib::toolchain;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "需要网络(国内镜像下载)"]
fn install_missing_managed_toolchain_from_mirror() {
    let _g = ENV_LOCK.lock().unwrap();
    let base = std::env::temp_dir().join(format!("dsh-toolchain-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
    std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));

    let hub = Arc::new(LogHub::new(
        base.join("state/logs/launcher.log"),
        Arc::new(|_| {}),
        true,
    ));
    let token = CancellationToken::new();

    // 1. 安装托管 Node(缺失时才下载)
    let tools_before = Tools {
        pnpm: None,
        git: None,
        dsh_node_dir: None,
        dsh_cli_entry: None,
        dsh_harness_root: None,
    };
    let report = toolchain::ensure_tool(
        &hub,
        toolchain::Tool::Node,
        &token,
        &|s| {
            println!("[stage] {s}");
        },
        &tools_before,
    )
    .expect("托管 Node 安装应成功(国内镜像)");
    for m in &report.messages {
        println!("[msg] {m}");
    }
    let (bin, ver) = runtime::resolve_dsh_node().expect("安装后应能解析托管 Node");
    assert!(bin.is_file(), "node bin 必须存在: {}", bin.display());
    assert!(runtime::node_in_range(&ver), "版本必须在 dsh 范围:{ver}");
    println!("[ok] managed node {ver} → {}", bin.display());

    // 2. 安装托管 pnpm(11.7.0,catalog 内)
    let tools_after_node = Tools {
        pnpm: None,
        git: None,
        dsh_node_dir: bin.parent().map(|p| p.to_path_buf()),
        dsh_cli_entry: None,
        dsh_harness_root: None,
    };
    let report2 = toolchain::ensure_tool(
        &hub,
        toolchain::Tool::Pnpm,
        &token,
        &|s| {
            println!("[stage] {s}");
        },
        &tools_after_node,
    )
    .expect("托管 pnpm 安装应成功");
    for m in &report2.messages {
        println!("[msg] {m}");
    }
    let pnpm = toolchain::resolve_pnpm(&tools_after_node).expect("托管 pnpm 应可解析");
    // 用托管 node + pnpm shim 运行自检
    // 注意:在仓库内运行 pnpm 会遵循该仓库 packageManager 语义(hand-off);
    // 这里在隔离目录运行以断言托管 pnpm 本体版本。
    let out = std::process::Command::new(&pnpm)
        .args(["--version"])
        .current_dir(&base)
        .env(
            "PATH",
            format!(
                "{}:{}",
                tools_after_node.dsh_node_dir.as_ref().unwrap().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("pnpm shim 应可执行");
    assert!(
        out.status.success(),
        "pnpm --version 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = String::from_utf8_lossy(&out.stdout);
    assert!(
        v.trim_start().starts_with("11.7.0"),
        "pnpm 版本应为 11.7.0,实际 {v}"
    );
    println!("[ok] managed pnpm → {v}");

    // 3. InstallationSnapshot 持久化
    let snap = dsh_launcher_lib::ops::load_installation();
    assert!(snap.node.is_some(), "node 应记录在 InstallationSnapshot");
    assert!(snap.pnpm.is_some(), "pnpm 应记录在 InstallationSnapshot");
    println!(
        "[ok] installation snapshot: node={:?} pnpm={:?}",
        snap.node.as_ref().map(|c| &c.version),
        snap.pnpm.as_ref().map(|c| &c.version)
    );

    let _ = std::fs::remove_dir_all(&base);
    println!("ALL OK — 真实国内镜像链路验证通过");
}
