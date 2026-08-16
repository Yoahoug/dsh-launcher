// dsh-launcher · M5 插件/技能真实流程集成测试(需真实 dsh 仓库 + pnpm/node,默认忽略)
//
// 运行:cargo test --test plugins_skills_flow -- --ignored --nocapture
// 覆盖(M1/M2 验收口径):
// - 组合视图:dump-config 解析行 + 来源层分类 + !!js 原样透出;
// - 写补丁:备份 → 原子写 → dump-config 校验;非法补丁自动回滚;
// - 启停 / 保存配置(整行替换)/ 重置;
// - 技能:managed CRUD + 外部路径围栏。
// 全部使用隔离的临时 DSH_HOME(不动真实 ~/.dsh 与上游仓库)。
#![cfg(unix)]
use dsh_launcher_lib::log_hub::LogHub;
use dsh_launcher_lib::services::plugins::{self, WriteCtx};
use dsh_launcher_lib::services::runtime::{self, Tools};
use dsh_launcher_lib::services::skills::{self, ScanCtx};
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn repo_path() -> Option<String> {
    if let Ok(p) = std::env::var("DSH_REPO") {
        return Some(p);
    }
    let home = std::env::var("HOME").ok()?;
    let p = format!("{home}/Desktop/deepseek-harness");
    std::path::Path::new(&p).join("package.json").is_file().then_some(p)
}

fn tools() -> Option<Tools> {
    let node = runtime::resolve_dsh_node()?;
    Some(Tools {
        pnpm: runtime::resolve_executable("pnpm").or_else(|| crate_resolve_pnpm()),
        git: runtime::resolve_executable("git"),
        dsh_node_dir: node.0.parent().map(std::path::PathBuf::from),
    })
}

fn crate_resolve_pnpm() -> Option<std::path::PathBuf> {
    dsh_launcher_lib::toolchain::resolve_pnpm(&Tools {
        pnpm: None,
        git: None,
        dsh_node_dir: None,
    })
}

fn hub(base: &std::path::Path) -> Arc<LogHub> {
    Arc::new(LogHub::new(
        base.join("launcher.log"),
        Arc::new(|_| {}),
        true,
    ))
}

/// 手工构造一个最小 profile(bundles 只含 dsh-base,补丁显式 `[]`)。
fn make_profile(base: &std::path::Path, name: &str) {
    let dir = base.join("dshhome").join("profiles").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        format!(
            r#"{{"name":"dsh-profile-{name}","private":true,"dependencies":{{}},"dsh":{{"profile":{{"bundles":["@deepseek-ai/dsh-base"]}}}}}}"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("cordis.patch.yml"), "[]\n").unwrap();
}

#[test]
#[ignore = "需要真实 dsh 仓库与 pnpm/node(dump-config 子进程)"]
fn plugin_snapshot_and_patch_write_pipeline() {
    let _g = ENV_LOCK.lock().unwrap();
    let (Some(repo), Some(tools)) = (repo_path(), tools()) else {
        eprintln!("跳过:仓库或工具不可用");
        return;
    };
    let base = std::env::temp_dir().join(format!("dsh-m5-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    make_profile(&base, "m5test");
    let dsh_home = base.join("dshhome").to_string_lossy().to_string();
    let log = hub(&base);

    // ── M1:组合视图(真实 dump-config) ──
    let snap = plugins::snapshot(&tools, &repo, &dsh_home, "m5test", "");
    assert!(snap.dump_error.is_none(), "{:?}", snap.dump_error);
    assert!(!snap.rows.is_empty(), "dump 应解析出 loader 行");
    let timer = snap.rows.iter().find(|r| r.id == "timer").expect("timer 行存在");
    assert_eq!(timer.layer, dsh_launcher_lib::contract::PluginLayer::Bundle);
    assert!(timer.enabled);
    // 含 !!js 的行(如 session-persistence-jsonl)标记 raw-yaml
    if let Some(js_row) = snap.rows.iter().find(|r| r.raw_block.contains("!!js")) {
        assert_eq!(
            js_row.config_source,
            dsh_launcher_lib::contract::ConfigSource::RawYaml
        );
        assert!(js_row.config.is_none());
    }

    let ctx = WriteCtx {
        tools: tools.clone(),
        repo_path: repo.clone(),
        dsh_home_setting: dsh_home.clone(),
    };

    // ── M2:启停(写 profile patch + 备份 + 校验) ──
    let r = plugins::set_enabled(&log, &ctx, "m5test", "timer", false).unwrap();
    assert!(r.validated, "{:?}", r.error);
    assert!(r.backup.is_some(), "写入前必须有备份");
    let patch_text = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    assert!(patch_text.contains("disabled: true"), "{patch_text}");
    // 备份文件存在
    let bak = base.join("dshhome/profiles/m5test").join(r.backup.unwrap());
    assert!(bak.is_file());

    // 再启用:disabled 行被移除(整行语义)
    let r2 = plugins::set_enabled(&log, &ctx, "m5test", "timer", true).unwrap();
    assert!(r2.validated);
    let patch_text2 = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    assert!(!patch_text2.contains("disabled: true"), "{patch_text2}");

    // ── M2:保存配置(整行替换,非深合并) ──
    let cfg = serde_json::json!({ "port": 9999 });
    let r3 = plugins::save_config(&log, &ctx, "m5test", "timer", &cfg, None).unwrap();
    assert!(r3.validated);
    let patch_text3 = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    assert!(patch_text3.contains("port: 9999"), "{patch_text3}");

    // ── M2:非法补丁 → dump-config 校验失败 → 自动回滚 ──
    let before = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    let bad = "- id: timer\n  config: [unclosed\n";
    let err = plugins::save_config(&log, &ctx, "m5test", "timer", &serde_json::json!({}), Some(bad))
        .expect_err("非法 YAML 必须被 dump-config 拦下并回滚");
    assert!(err.contains("已自动回滚"), "{err}");
    let after = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    assert_eq!(before, after, "校验失败后必须恢复备份内容");

    // ── M2:重置(移除用户条目) ──
    let r4 = plugins::reset_row(&log, &ctx, "m5test", "timer").unwrap();
    assert!(r4.validated);
    let patch_text4 = std::fs::read_to_string(base.join("dshhome/profiles/m5test/cordis.patch.yml")).unwrap();
    assert!(!patch_text4.contains("port: 9999"), "{patch_text4}");

    // 仅校验(不写文件)
    let v = plugins::validate_patch(&ctx, "m5test");
    assert!(v.ok && v.validated, "{:?}", v.error);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
#[ignore = "需要真实 dsh 仓库与 pnpm/node"]
fn skills_managed_crud_and_fence() {
    let _g = ENV_LOCK.lock().unwrap();
    let (Some(repo), _tools) = (repo_path(), tools()) else {
        eprintln!("跳过:仓库或工具不可用");
        return;
    };
    let base = std::env::temp_dir().join(format!("dsh-m5-skills-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let log = hub(&base);
    let ctx = ScanCtx {
        repo_path: repo,
        dsh_home_setting: base.join("dshhome").to_string_lossy().to_string(),
        skill_managed_root_setting: String::new(),
        external_skill_roots: vec![],
    };

    // 创建 → 扫描可见 → 更新 → 删除
    let s = skills::create(&log, &ctx, "m5-integration", "集成测试技能", None, "正文").unwrap();
    assert_eq!(s.name, "m5-integration");
    let snap = skills::snapshot(&ctx, &log, "m5test");
    assert!(snap.skills.iter().any(|x| x.name == "m5-integration"));
    let u = skills::update(&log, &ctx, "m5-integration", "新描述", Some("when"), "新正文").unwrap();
    assert_eq!(u.description, "新描述");
    skills::delete(&log, &ctx, "m5-integration").unwrap();
    let snap2 = skills::snapshot(&ctx, &log, "m5test");
    assert!(!snap2.skills.iter().any(|x| x.name == "m5-integration"));

    // 路径围栏:根外技能一律拒绝
    let outside = base.join("outside/evil");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("SKILL.md"),
        "---\nname: evil\ndescription: x\n---\nbody",
    )
    .unwrap();
    assert!(skills::delete(&log, &ctx, "evil").is_err());
    assert!(outside.join("SKILL.md").is_file(), "外部文件不得被删");

    let _ = std::fs::remove_dir_all(&base);
}
