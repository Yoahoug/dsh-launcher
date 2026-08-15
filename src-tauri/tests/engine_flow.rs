// dsh-launcher · M0 场景移植集成测试(真实进程,无 mock)
// 场景1:spawn_web → 就绪行 → wait_ready → 端口确认 → 优雅停止;
// 场景2:端口占用检测 probe_port / port_holder_pid。
// 全部使用隔离的 DSH_LAUNCHER_STATE_DIR;env 覆盖需串行(ENV_LOCK)。
use dsh_launcher_lib::config::{self, state_dir};
use dsh_launcher_lib::log_hub::LogHub;
use dsh_launcher_lib::services::runtime::Tools;
use dsh_launcher_lib::services::supervisor::Supervisor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// fake pnpm:仅实现 `dsh web --port N`,契约与真实 dsh 一致
/// (输出就绪行 `dsh web: http://127.0.0.1:N/`,并在该端口起 http.server)。
fn fake_bin_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dsh-fakebin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pnpm = dir.join("pnpm");
    std::fs::write(
        &pnpm,
        r#"#!/bin/bash
# fake pnpm:仅支持 `dsh web --port N`(真实 dsh 契约)
if [ "$1" = "dsh" ] && [ "$2" = "web" ]; then
  shift 2
  port="3080"
  while [ $# -gt 0 ]; do
    case "$1" in
      --port) port="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 &
  echo "dsh web: http://127.0.0.1:$port/"
  wait
fi
exit 0
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&pnpm, std::fs::Permissions::from_mode(0o755));
    }
    dir
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

fn make_tools() -> (Tools, std::path::PathBuf) {
    let dir = fake_bin_dir();
    let tools = Tools {
        pnpm: Some(dir.join("pnpm")),
        git: None,
        dsh_node_dir: None,
    };
    (tools, dir)
}

/// 场景1:dsh web 完整生命周期(启动 → 就绪 → 停止)。
#[test]
fn spawn_web_ready_then_stop() {
    let _g = ENV_LOCK.lock().unwrap();
    let base = std::env::temp_dir().join(format!("dsh-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("state")).unwrap();
    std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
    std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));

    let hub = Arc::new(LogHub::new(
        base.join("state/logs/launcher.log"),
        Arc::new(|_| {}),
        true,
    ));
    let sup = Supervisor::new(hub.clone());
    let (tools, _fake) = make_tools();
    let cwd = base.join("repo");
    std::fs::create_dir_all(&cwd).unwrap();

    let port = free_port();
    let pid = sup
        .spawn_web(
            &tools,
            &cwd.display().to_string(),
            port,
            "127.0.0.1",
            "",
            |_| {},
        )
        .expect("spawn_web 应成功");

    // wait_ready:就绪行 → 端口确认
    let url = sup
        .wait_ready(pid, port, 20_000)
        .expect("应出现就绪行且端口可连接");
    assert!(
        url.contains(&format!("http://127.0.0.1:{port}")),
        "就绪 URL 应指向配置端口: {url}"
    );

    // 停止:优雅 → 5s → 强杀;进程必须消失
    let out = sup.stop("dsh web");
    assert_ne!(
        out,
        dsh_launcher_lib::services::supervisor::StopOutcome::Missing
    );
    for _ in 0..50 {
        if !Supervisor::is_alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!Supervisor::is_alive(pid), "stop 后 dsh web 进程应已退出");

    // 幂等:再次 stop 返回 Missing
    assert_eq!(
        sup.stop("dsh web"),
        dsh_launcher_lib::services::supervisor::StopOutcome::Missing
    );
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&_fake);
}

/// 场景2:端口占用检测(先占端口 → probe_port true;释放 → false)。
#[test]
fn probe_port_detects_occupation() {
    let _g = ENV_LOCK.lock().unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(config::probe_port("127.0.0.1", port), "占用端口应被探测到");
    drop(l);
    // 释放后短暂等待内核释放
    std::thread::sleep(Duration::from_millis(300));
    assert!(!config::probe_port("127.0.0.1", port), "释放后不应再探测到");
}

/// 场景2b:port_holder_pid 诊断能识别占用者。
#[test]
fn port_holder_diagnostics() {
    let _g = ENV_LOCK.lock().unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let holder = dsh_launcher_lib::services::supervisor::port_holder_pid(port);
    assert!(holder.is_some(), "占用端口应有持有者 pid");
    assert!(Supervisor::is_alive(holder.unwrap()), "持有者应存活");
}

/// 场景3:detach 语义(清空注册表,不杀进程;进程仍存活)。
#[test]
fn detach_keeps_child_alive() {
    let _g = ENV_LOCK.lock().unwrap();
    let base = std::env::temp_dir().join(format!("dsh-detach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("state")).unwrap();
    std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
    std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));

    let hub = Arc::new(LogHub::new(
        base.join("state/logs/launcher.log"),
        Arc::new(|_| {}),
        true,
    ));
    let sup = Supervisor::new(hub.clone());
    let (tools, _fake) = make_tools();
    let cwd = base.join("repo");
    std::fs::create_dir_all(&cwd).unwrap();

    let port = free_port();
    let pid = sup
        .spawn_web(
            &tools,
            &cwd.display().to_string(),
            port,
            "127.0.0.1",
            "",
            |_| {},
        )
        .unwrap();
    let url = sup.wait_ready(pid, port, 20_000).unwrap();
    assert!(url.contains("http://127.0.0.1"));

    // detach:注册表清空,子进程继续运行
    sup.detach();
    assert_eq!(sup.web_pid(), None, "detach 后注册表应清空");
    assert!(Supervisor::is_alive(pid), "detach 后 dsh web 应继续运行");

    // 清理:杀进程树(进程组)
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    for _ in 0..50 {
        if !Supervisor::is_alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&_fake);
}

/// 场景4:runtime.json 持久化 → recall 召回记录。
#[test]
fn persist_and_recall_running() {
    let _g = ENV_LOCK.lock().unwrap();
    let base = std::env::temp_dir().join(format!("dsh-recall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("state")).unwrap();
    std::env::set_var("DSH_LAUNCHER_STATE_DIR", base.join("state"));
    std::env::set_var("DSH_LAUNCHER_CONFIG_DIR", base.join("config"));

    let hub = Arc::new(LogHub::new(
        base.join("state/logs/launcher.log"),
        Arc::new(|_| {}),
        true,
    ));
    let sup = Supervisor::new(hub.clone());
    let (tools, _fake) = make_tools();
    let cwd = base.join("repo");
    std::fs::create_dir_all(&cwd).unwrap();

    let port = free_port();
    let pid = sup
        .spawn_web(
            &tools,
            &cwd.display().to_string(),
            port,
            "127.0.0.1",
            "",
            |_| {},
        )
        .unwrap();
    let _ = sup.wait_ready(pid, port, 20_000).unwrap();
    sup.persist_running();

    // runtime.json 落盘
    let path = state_dir().join("runtime.json");
    assert!(path.exists(), "runtime.json 应存在");
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains(&format!("\"webPid\": {pid}")),
        "应记录 webPid: {raw}"
    );

    // recall:进程存活时重建记录
    let sup2 = Supervisor::new(hub.clone());
    let m = sup2.recall();
    assert!(m.is_some(), "存活进程应被召回");
    assert_eq!(m.unwrap().pid, pid);

    // 停止进程后 recall 返回 None
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    for _ in 0..50 {
        if !Supervisor::is_alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let sup3 = Supervisor::new(hub.clone());
    assert!(sup3.recall().is_none(), "进程退出后 recall 应为 None");
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&_fake);
}
