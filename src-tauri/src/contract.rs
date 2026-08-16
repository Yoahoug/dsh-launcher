// dsh-launcher · 前后端共享契约(Rust 侧定义,与 src-ui/src/types/schema.ts 对齐)
// 变更时需同步两侧,并通过 contract tests / 手工核对保持不漂移。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LauncherState {
    Idle,
    Syncing,
    Installing,
    Building,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LauncherMode {
    None,
    Normal,
    Dev,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
    Err,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSummary {
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoSnapshot {
    pub branch: String,
    pub head: String,
    pub behind: i64,
    pub ahead: i64,
    pub dirty: bool,
    pub dirty_files: u64,
    pub sync_at: Option<i64>,
    pub remote_up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    pub mode: Option<String>,
    pub checking: bool,
    pub available: bool,
    pub version: Option<String>,
    pub url: Option<String>,
    pub size: Option<u64>,
    pub notes: Option<String>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub installing: bool,
    pub progress: Option<u8>,
}

/// 与 src/state.mjs `state` 对象对齐的完整快照。
/// 长任务种类。
/// exclusive-write 分组(安装/克隆/构建/更新/自更新)同一时间只能运行一个;
/// start/dev 与 exclusive-write 互斥;stop/cancel 始终可发起。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    InstallNode,
    InstallGit,
    InstallPnpm,
    InstallToolchain,
    CloneRepo,
    FullSetup,
    InstallDeps,
    Build,
    UpdateRebuild,
    RebuildRestart,
    StartWeb,
    StartDev,
    StopAll,
    SelfUpdate,
    /// M5:从 dsh-plugins 安装插件包(pnpm install + build + dsh plugin add)。
    PluginInstall,
    /// M5:移除插件包(dsh plugin remove)。
    PluginRemove,
}

impl OperationKind {
    /// 是否属于 exclusive-write(同一时间只能运行一个)。
    pub fn is_exclusive_write(self) -> bool {
        matches!(
            self,
            OperationKind::InstallNode
                | OperationKind::InstallGit
                | OperationKind::InstallPnpm
                | OperationKind::InstallToolchain
                | OperationKind::CloneRepo
                | OperationKind::FullSetup
                | OperationKind::InstallDeps
                | OperationKind::Build
                | OperationKind::UpdateRebuild
                | OperationKind::RebuildRestart
                | OperationKind::SelfUpdate
                | OperationKind::PluginInstall
                | OperationKind::PluginRemove
        )
    }

    /// 中文动作名(日志/UI 提示)。
    pub fn label(self) -> &'static str {
        match self {
            OperationKind::InstallNode => "安装 Node",
            OperationKind::InstallGit => "安装 Git",
            OperationKind::InstallPnpm => "安装 pnpm",
            OperationKind::InstallToolchain => "安装工具链",
            OperationKind::CloneRepo => "克隆仓库",
            OperationKind::FullSetup => "一键安装",
            OperationKind::InstallDeps => "安装依赖",
            OperationKind::Build => "构建",
            OperationKind::UpdateRebuild => "更新并构建",
            OperationKind::RebuildRestart => "重建并重启",
            OperationKind::StartWeb => "启动 dsh",
            OperationKind::StartDev => "启动开发模式",
            OperationKind::StopAll => "停止",
            OperationKind::SelfUpdate => "应用自更新",
            OperationKind::PluginInstall => "安装插件",
            OperationKind::PluginRemove => "移除插件",
        }
    }
}

/// 操作状态:只有 Success 才是终态成功;Failed/Cancelled/Interrupted 均为终态失败。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
    Interrupted,
}

impl OperationStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OperationStatus::Success
                | OperationStatus::Failed
                | OperationStatus::Cancelled
                | OperationStatus::Interrupted
        )
    }
}

/// 单个长任务的可见快照(写入 AppSnapshot.operation)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    pub operation_id: u64,
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub stage: String,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub cancellable: bool,
}

/// 被动作矩阵禁用的动作及其具体原因(UI 按钮禁用时展示)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisabledAction {
    pub action: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub version: String,
    pub state: LauncherState,
    pub mode: LauncherMode,
    pub phase: String,
    pub error: Option<ErrorSummary>,
    pub url: Option<String>,
    pub web_pid: Option<u32>,
    pub dev_pid: Option<u32>,
    pub started_at: Option<i64>,
    pub ready_at: Option<i64>,
    pub hmr_active: bool,
    pub repo: RepoSnapshot,
    pub busy: bool,
    pub launcher_pid: u32,
    pub update: UpdateSnapshot,
    /// 当前长任务(无任务时 None)。UI 只在 status == success 时显示成功。
    pub operation: Option<OperationSnapshot>,
    /// 动作矩阵:被禁用的动作与原因。
    pub disabled_actions: Vec<DisabledAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub ts: i64,
    pub src: String,
    pub level: LogLevel,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub logs: Vec<LogEntry>,
    pub sources: Vec<String>,
}

/// 设置(与 src/config.mjs DEFAULTS 对齐)。engine 行为,由 Node daemon 持久化。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub repo_path: String,
    pub port: u16,
    pub host: String,
    pub dsh_home: String,
    pub autostart: bool,
    pub open_browser: bool,
    pub auto_update_check: bool,
    pub build_args: String,
    pub ready_timeout_ms: u64,
    pub start_timeout_ms: u64,
    /// 首次运行是否已处理(跳过或完成)。为 true 时不再展示首次运行向导,
    /// 即使仓库当前不可用(用户可在启动器内随时克隆/配置)。
    pub first_run_skipped: bool,
    /// M5:插件/技能子界面的目标 profile(默认 'web',对齐 dsh web 别名)。
    pub profile_name: String,
    /// M5:dsh-plugins 仓库根;空 = 自动探测 profile deps 里的 file: 链接。
    pub dsh_plugins_path: String,
    /// M5:技能扫描的自定义根目录(内置映射之外追加)。
    pub external_skill_roots: Vec<String>,
    /// M5:managed 技能根;空 = $DSH_HOME/skills。
    pub skill_managed_root: String,
}

/// 主题偏好。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// 关闭窗口行为。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    #[default]
    Tray,
    Quit,
}

/// 桌面偏好:仅由 Rust 持久化,不写入 Node 配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub theme: Theme,
    pub close_behavior: CloseBehavior,
    pub launch_on_startup: bool,
    pub silent_startup: bool,
    pub show_tray_icon: bool,
    pub confirm_stop_and_quit: bool,
}

/// 桌面信息:偏好 + 首次运行状态(供 First-run 流程判定)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshot {
    pub preferences: DesktopPreferences,
    pub first_run_done: bool,
    pub version: String,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            close_behavior: CloseBehavior::default(),
            launch_on_startup: false,
            silent_startup: false,
            show_tray_icon: true,
            confirm_stop_and_quit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoUsable {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 工具链来源:系统安装 / Launcher 托管 / 项目本地·Corepack。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    /// 系统安装(PATH / Homebrew / nvm / volta / fnm 等,非 Launcher 管理)。
    System,
    /// Launcher 托管(签名 catalog 下载,版本化目录,子进程 PATH 注入)。
    Managed,
    /// 项目本地 / Corepack(路径含 corepack)。
    Corepack,
}

/// 工具链组件检测状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolCheck {
    /// 存在且自检通过(版本可用)。
    Detected,
    /// 存在但版本不兼容/无法读取版本。
    Incompatible,
    /// 所有来源(系统/托管/项目)均不存在。
    Missing,
}

/// 单个工具链组件的运行时快照(当前实际生效;版本/来源/路径/检测状态)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRuntime {
    /// 实际生效版本(如 v24.9.0 / 11.21.0 / 2.47.0);None = 未安装。
    pub version: Option<String>,
    /// 来源;None = 未安装。
    pub source: Option<ToolSource>,
    /// 实际生效可执行文件绝对路径;None = 未安装。
    pub path: Option<String>,
    /// 检测状态。
    pub status: ToolCheck,
    /// 是否经签名 catalog 下载并 SHA-256 校验。仅托管工具可为 true;系统工具恒 false。
    pub verified: bool,
    /// 明确提示/推荐(不兼容或缺失时给出可执行建议)。
    pub hint: Option<String>,
    /// 是否存在可切换的托管版本(catalog 有当前平台条目)。
    pub managed_available: bool,
}

impl ToolRuntime {
    /// 是否缺失(所有来源均不存在)。
    pub fn is_missing(&self) -> bool {
        self.status == ToolCheck::Missing
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub repo_path: String,
    pub repo_usable: RepoUsable,
    pub dist_built: Option<bool>,
    /// 当前生效平台(macos / windows / linux;前端据此决定 MinGit 是否展示)。
    pub platform: String,
    pub node: ToolRuntime,
    pub pnpm: ToolRuntime,
    pub git: ToolRuntime,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionAccepted {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aborted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── 主窗口内 DeepSeek 工作区(M4.1:dsh-content 子 WebView) ─────

/// 主窗口内的工作区。launcher = 启动器管理界面;dsh = DeepSeek 完整工作区
/// (主窗口内的子 WebView,不是独立窗口、不是 iframe)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    #[default]
    Launcher,
    Dsh,
}

/// dsh-content 子 WebView 生命周期状态。
/// 只有 Ready 才允许显示 DeepSeek 工作区;Disconnected/Failed 必须显示断线/错误状态。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DshViewStatus {
    /// 尚未创建子 WebView。
    NotCreated,
    /// 正在创建(异步,可能在等待服务就绪)。
    Creating,
    /// 已创建,正在加载 DSH 页面。
    Loading,
    /// 页面加载完成且健康检查通过(唯一可展示工作区状态)。
    Ready,
    /// DSH 服务意外退出,与子 WebView 断开(可重连)。
    Disconnected,
    /// 创建/加载失败(可重试)。
    Failed,
}

impl DshViewStatus {
    /// 当前是否处于「可展示子 WebView」状态(隐藏/显示切换依据)。
    pub fn is_showable(self) -> bool {
        matches!(self, DshViewStatus::Loading | DshViewStatus::Ready)
    }

    /// 终态失败(重试前停留在该状态)。
    pub fn is_terminal_failure(self) -> bool {
        matches!(self, DshViewStatus::Disconnected | DshViewStatus::Failed)
    }
}

/// DeepSeek 工作区与子 WebView 的全量快照(事件 app://dsh-view-state + get_dsh_view_state)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DshViewSnapshot {
    pub workspace: Workspace,
    pub status: DshViewStatus,
    pub url: Option<String>,
    pub error: Option<String>,
    /// 是否存在「成功后自动进入 DeepSeek」的 pending 意图(accepted ≠ success)。
    pub pending_enter: bool,
    /// 是否可以返回启动器(workspace 为 dsh 时恒 true)。
    pub can_back_to_launcher: bool,
    /// 是否可以重试/重连。
    pub can_retry: bool,
    pub can_reconnect: bool,
}

/// 事件名(与前端 EVENTS 常量对齐)。M2 bridge 开始使用,届时移除 allow。
pub const EVENT_STATE_CHANGED: &str = "app://state-changed";
pub const EVENT_LOG_APPENDED: &str = "app://log-appended";
/// 托盘要求 renderer 打开某页面(取值 dashboard|logs|settings)。
pub const EVENT_OPEN_PAGE: &str = "app://open-page";
/// 桌面偏好已变更(Renderer 应用主题等)。
pub const EVENT_PREFERENCES_CHANGED: &str = "app://preferences-changed";
/// DeepSeek 工作区/子 WebView 状态变更。
pub const EVENT_DSH_VIEW_STATE: &str = "app://dsh-view-state";
/// M5:技能写操作(新建/编辑/删除/导入/一键启用)后广播,前端刷新技能页。
pub const EVENT_SKILLS_CHANGED: &str = "app://skills-changed";

// ── M5:插件管理子界面(官方插件管理增强版) ────────────────

/// profile 摘要(插件页 profile 选择器)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub name: String,
    /// 有序组合包列表(dsh.profile.bundles)。
    pub bundles: Vec<String>,
    /// dependencies:包名 → spec(file:/git:/版本)。
    pub deps: std::collections::BTreeMap<String, String>,
    /// profile 的 cordis.patch.yml 是否可读。
    pub patch_ok: bool,
}

/// 插件行来源层。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PluginLayer {
    /// 组合包层(profile bundles 声明)。
    Bundle,
    /// profile 自己的 cordis.patch.yml。
    ProfilePatch,
    /// $DSH_HOME/cordis.patch.yml(home 级)。
    HomePatch,
    /// --patch <path> argv overlay(不持久,只读)。
    Overlay,
}

/// config 来源:'dump' = 可表单化;'raw-yaml' = 含 !!js 表达式,锁定原始 YAML。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigSource {
    Dump,
    RawYaml,
}

/// 组合后的一个 loader 行(与方案 §4.2 对齐;rawBlock/inUserPatch 为实现补充)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginRow {
    pub id: String,
    /// name 字段(包导出名,如 @deepseek-ai/dsh-llm)。
    pub module: String,
    pub layer: PluginLayer,
    /// 来源层展示文本(如 @deepseek-ai/dsh-base / patch 绝对路径)。
    pub layer_label: String,
    /// 该行是否已在用户 profile patch 中存在条目(重置按钮可用性)。
    pub in_user_patch: bool,
    /// 有无 disabled 生效。
    pub enabled: bool,
    /// 组合后的 config(dump-config 不求值 !!js;含 !!js 时为 None)。
    pub config: Option<serde_json::Value>,
    pub config_source: ConfigSource,
    /// 原始 YAML 块(整行,从 `- id:` 到该行末尾;raw-yaml 编辑/预览用)。
    pub raw_block: String,
    /// bundle/home-patch 行可经覆盖编辑(整行重述);overlay 行不可编辑。
    pub editable: bool,
    /// 包说明(dsh-plugins 包匹配时来自其 package.json;否则 None)。
    pub description: Option<String>,
}

/// dsh-plugins 仓库里的包。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DshPluginPackage {
    pub dir: String,
    pub abs_dir: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// 是否声明 dsh.bundle(bundle 安装后自动激活其 patch 层)。
    pub is_bundle: bool,
    pub patch_file: Option<String>,
    /// 已安装到的 profile 列表。
    pub installed_in: Vec<String>,
}

/// 补丁写入结果(备份 + dump-config 校验)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatchWriteResult {
    /// 备份文件名(cordis.patch.yml.bak-<ts>);未发生写动作为 None。
    pub backup: Option<String>,
    pub ok: bool,
    pub summary: String,
    /// dump-config 校验是否通过(通过即运行中 dsh web 可 HMR 生效)。
    pub validated: bool,
    pub error: Option<String>,
}

/// 插件组合视图快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginsSnapshot {
    pub profiles: Vec<ProfileSummary>,
    pub rows: Vec<PluginRow>,
    pub packages: Vec<DshPluginPackage>,
    /// 生效的 dsh-plugins 仓库根(设置值或 profile deps 自动探测值)。
    pub plugins_path: Option<String>,
    /// 当前生效 profile;None = 不存在/未指定。
    pub profile: Option<String>,
    /// dump-config 失败诊断(此时 rows 为空;UI 展示警示条)。
    pub dump_error: Option<String>,
}

// ── M5:技能管理子界面 ────────────────────────────────────

/// 技能来源分组。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillSource {
    /// $DSH_HOME/skills(本启动器直接管理)。
    Managed,
    /// ~/.codex/skills。
    Codex,
    /// ~/.claude/skills。
    Claude,
    /// Cursor 系统技能不纳入 launcher 扫描;其余外部工具根按实现扫描。
    Cursor,
    /// ~/.config/opencode/skills。
    Opencode,
    /// ~/.agents/skills(dsh 原生已扫,这里只展示与导入)。
    Agents,
    /// <repo_path>/.dsh/skills、.agents/skills(只读展示)。
    Project,
    /// 设置 externalSkillRoots 追加的自定义根。
    Custom,
}

/// 单个技能摘要。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub model_invocable: bool,
    pub user_invocable: bool,
    pub source: SkillSource,
    /// 技能所在目录(目录包)或根目录(扁平 md)。
    pub dir: String,
    /// SKILL.md / <name>.md 的绝对路径。
    pub path: String,
    pub size_bytes: u64,
    /// 目录包含 scripts/references 等附带资源。
    pub has_scripts: bool,
}

/// 扫描根目录描述。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillRoot {
    pub key: String,
    pub label: String,
    pub path: String,
    pub exists: bool,
    pub managed: bool,
    /// 该根是否已写进目标 profile 的 skill-filesystem.customSkillDirs(一键启用状态)。
    pub enabled: bool,
}

/// 技能快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsSnapshot {
    pub roots: Vec<SkillRoot>,
    pub skills: Vec<SkillSummary>,
    /// 目标 profile 是否已安装 skill-external-roots 插件。
    pub plugins_installed: bool,
    /// 被跳过条目与原因(UI 展示"N 个被跳过")。
    pub skipped: Vec<String>,
}

/// 运行中 dsh 插件回写的「实际注入」技能清单条目(skills-active.json)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSkill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    /// 来源桶(插件固定 'external')。
    pub source: String,
    /// 该技能所在根目录(绝对路径)。
    pub root: String,
    pub path: String,
    pub model_invocable: bool,
    pub user_invocable: bool,
}

/// 已启动技能清单快照(技能页「已启动」子界面)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsActiveSnapshot {
    pub file: String,
    pub written_at: Option<i64>,
    pub skills: Vec<ActiveSkill>,
    /// 读取/解析失败诊断(null = 正常)。
    pub error: Option<String>,
    /// 目标 profile 补丁里 skill-external-roots 行配置的 skillControlFile。
    pub control_file: Option<String>,
    pub control_file_exists: bool,
}

/// 注入控制文件状态(启动器写,插件读)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsControlState {
    pub file: String,
    pub version: u32,
    pub roots: std::collections::BTreeMap<String, bool>,
    pub skills: std::collections::BTreeMap<String, bool>,
}

/// 注入开关写入结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillToggleResult {
    pub ok: bool,
    pub summary: String,
    pub enabled: bool,
}

impl AppSnapshot {
    /// M1 mock:空闲快照(M2 由 bridge 提供真实数据)。
    pub fn mock_idle() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            state: LauncherState::Idle,
            mode: LauncherMode::None,
            phase: String::new(),
            error: None,
            url: None,
            web_pid: None,
            dev_pid: None,
            started_at: None,
            ready_at: None,
            hmr_active: false,
            repo: RepoSnapshot {
                branch: String::new(),
                head: String::new(),
                behind: -1,
                ahead: -1,
                dirty: false,
                dirty_files: 0,
                sync_at: None,
                remote_up_to_date: true,
            },
            busy: false,
            launcher_pid: std::process::id(),
            update: UpdateSnapshot {
                mode: None,
                checking: false,
                available: false,
                version: None,
                url: None,
                size: None,
                notes: None,
                message: None,
                error: None,
                installing: false,
                progress: None,
            },
            operation: None,
            disabled_actions: Vec::new(),
        }
    }
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        let home = crate::config::home_dir();
        Self {
            repo_path: format!("{home}/Desktop/deepseek-harness"),
            port: 3080,
            host: "127.0.0.1".into(),
            dsh_home: String::new(),
            autostart: false,
            open_browser: true,
            auto_update_check: true,
            build_args: String::new(),
            ready_timeout_ms: 180_000,
            start_timeout_ms: 180_000,
            first_run_skipped: false,
            profile_name: "web".into(),
            dsh_plugins_path: String::new(),
            external_skill_roots: Vec::new(),
            skill_managed_root: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前后端契约抽查:M1 前端 mock 快照与 Rust 结构互转。
    #[test]
    fn snapshot_serde_roundtrip() {
        let s = AppSnapshot::mock_idle();
        let json = serde_json::to_string(&s).unwrap();
        let back: AppSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // 字段命名必须 camelCase(与 TS schema 一致)
        assert!(json.contains("\"webPid\""));
        assert!(json.contains("\"launcherPid\""));
        assert!(json.contains("\"startedAt\""));
        assert!(json.contains("\"remoteUpToDate\""));
        let s2 = SettingsSnapshot::default();
        let j2 = serde_json::to_string(&s2).unwrap();
        assert!(j2.contains("\"readyTimeoutMs\""));
        assert!(j2.contains("\"repoPath\""));
        assert!(j2.contains("\"autoUpdateCheck\""));
    }

    #[test]
    fn action_payload_shape() {
        let ok = ActionAccepted {
            ok: true,
            reason: None,
            aborted: None,
            already: None,
        };
        let j = serde_json::to_string(&ok).unwrap();
        assert_eq!(j, r#"{"ok":true}"#);
    }

    #[test]
    fn state_serde_lowercase() {
        assert_eq!(
            serde_json::to_string(&LauncherState::Running).unwrap(),
            r#""running""#
        );
        assert_eq!(
            serde_json::to_string(&LauncherMode::Dev).unwrap(),
            r#""dev""#
        );
    }

    #[test]
    fn operation_kind_exclusive_write_groups() {
        assert!(OperationKind::InstallNode.is_exclusive_write());
        assert!(OperationKind::CloneRepo.is_exclusive_write());
        assert!(OperationKind::FullSetup.is_exclusive_write());
        assert!(OperationKind::UpdateRebuild.is_exclusive_write());
        assert!(OperationKind::SelfUpdate.is_exclusive_write());
        assert!(OperationKind::PluginInstall.is_exclusive_write());
        assert!(OperationKind::PluginRemove.is_exclusive_write());
        assert!(!OperationKind::StartWeb.is_exclusive_write());
        assert!(!OperationKind::StartDev.is_exclusive_write());
        assert!(!OperationKind::StopAll.is_exclusive_write());
    }

    #[test]
    fn plugin_and_skill_types_serde_camel_and_kebab() {
        // 插件行:layer/configSource 必须 kebab-case,其余 camelCase
        let row = PluginRow {
            id: "web".into(),
            module: "@deepseek-ai/dsh-web".into(),
            layer: PluginLayer::ProfilePatch,
            layer_label: "/Users/u/.dsh/profiles/web/cordis.patch.yml".into(),
            in_user_patch: true,
            enabled: true,
            config: Some(serde_json::json!({ "searchProvider": "tavily" })),
            config_source: ConfigSource::Dump,
            raw_block: "- id: web\n  config:\n    searchProvider: tavily\n".into(),
            editable: true,
            description: Some("x".into()),
        };
        let j = serde_json::to_string(&row).unwrap();
        assert!(j.contains("\"layer\":\"profile-patch\""), "{j}");
        assert!(j.contains("\"configSource\":\"dump\""), "{j}");
        assert!(j.contains("\"inUserPatch\":true"), "{j}");
        assert!(j.contains("\"rawBlock\""), "{j}");
        let back: PluginRow = serde_json::from_str(&j).unwrap();
        assert_eq!(back, row);

        // 技能来源
        assert_eq!(
            serde_json::to_string(&SkillSource::Managed).unwrap(),
            r#""managed""#
        );
        assert_eq!(
            serde_json::to_string(&SkillSource::Codex).unwrap(),
            r#""codex""#
        );
        let skill = SkillSummary {
            name: "foo-bar".into(),
            description: "d".into(),
            when_to_use: None,
            model_invocable: true,
            user_invocable: true,
            source: SkillSource::Codex,
            dir: "/x/y".into(),
            path: "/x/y/SKILL.md".into(),
            size_bytes: 12,
            has_scripts: true,
        };
        let js = serde_json::to_string(&skill).unwrap();
        assert!(js.contains("\"whenToUse\":null"), "{js}");
        assert!(js.contains("\"modelInvocable\":true"), "{js}");
        assert!(js.contains("\"hasScripts\":true"), "{js}");
        let back: SkillSummary = serde_json::from_str(&js).unwrap();
        assert_eq!(back, skill);
    }

    #[test]
    fn skills_changed_event_constant() {
        assert_eq!(EVENT_SKILLS_CHANGED, "app://skills-changed");
    }

    #[test]
    fn active_and_control_types_serde() {
        let active = SkillsActiveSnapshot {
            file: "/Users/u/.dsh/state/skills-active.json".into(),
            written_at: Some(1723800000000),
            skills: vec![ActiveSkill {
                name: "tavily-extract".into(),
                description: "d".into(),
                when_to_use: Some("w".into()),
                source: "external".into(),
                root: "/Users/u/.claude/skills".into(),
                path: "/Users/u/.claude/skills/tavily-extract/SKILL.md".into(),
                model_invocable: true,
                user_invocable: true,
            }],
            error: None,
            control_file: Some("/Users/u/.dsh/skills-control.json".into()),
            control_file_exists: true,
        };
        let j = serde_json::to_string(&active).unwrap();
        assert!(j.contains("\"writtenAt\""), "{j}");
        assert!(j.contains("\"whenToUse\":\"w\""), "{j}");
        assert!(j.contains("\"modelInvocable\":true"), "{j}");
        assert!(j.contains("\"controlFileExists\":true"), "{j}");
        let back: SkillsActiveSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, active);

        let ctl = SkillsControlState {
            file: "/Users/u/.dsh/skills-control.json".into(),
            version: 1,
            roots: std::collections::BTreeMap::from([("codex".into(), true)]),
            skills: std::collections::BTreeMap::from([("win-host".into(), false)]),
        };
        let cj = serde_json::to_string(&ctl).unwrap();
        assert!(cj.contains("\"version\":1"), "{cj}");
        let back: SkillsControlState = serde_json::from_str(&cj).unwrap();
        assert_eq!(back, ctl);
    }

    #[test]
    fn operation_snapshot_serde_camel() {
        let op = OperationSnapshot {
            operation_id: 7,
            kind: OperationKind::CloneRepo,
            status: OperationStatus::Running,
            stage: "克隆中…".into(),
            progress: Some(42),
            error: None,
            started_at: Some(1),
            finished_at: None,
            cancellable: true,
        };
        let j = serde_json::to_string(&op).unwrap();
        assert!(j.contains("\"operationId\":7"), "{j}");
        assert!(j.contains("\"kind\":\"clone_repo\""), "{j}");
        assert!(j.contains("\"status\":\"running\""), "{j}");
        let back: OperationSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, op);
        assert!(!OperationStatus::Running.is_terminal());
        assert!(OperationStatus::Success.is_terminal());
        assert!(OperationStatus::Cancelled.is_terminal());
        assert!(OperationStatus::Interrupted.is_terminal());
    }

    #[test]
    fn dsh_view_snapshot_serde_camel() {
        let s = DshViewSnapshot {
            workspace: Workspace::Dsh,
            status: DshViewStatus::Ready,
            url: Some("http://127.0.0.1:3080/".into()),
            error: None,
            pending_enter: false,
            can_back_to_launcher: true,
            can_retry: true,
            can_reconnect: true,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"workspace\":\"dsh\""), "{j}");
        assert!(j.contains("\"status\":\"ready\""), "{j}");
        assert!(j.contains("\"pendingEnter\":false"), "{j}");
        assert!(j.contains("\"canBackToLauncher\":true"), "{j}");
        let back: DshViewSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn dsh_view_status_serde_and_predicates() {
        assert_eq!(
            serde_json::to_string(&DshViewStatus::NotCreated).unwrap(),
            r#""not_created""#
        );
        assert_eq!(
            serde_json::to_string(&DshViewStatus::Disconnected).unwrap(),
            r#""disconnected""#
        );
        assert!(DshViewStatus::Loading.is_showable());
        assert!(DshViewStatus::Ready.is_showable());
        assert!(!DshViewStatus::NotCreated.is_showable());
        assert!(!DshViewStatus::Creating.is_showable());
        assert!(DshViewStatus::Disconnected.is_terminal_failure());
        assert!(DshViewStatus::Failed.is_terminal_failure());
        assert!(!DshViewStatus::Ready.is_terminal_failure());
        assert_eq!(
            serde_json::to_string(&Workspace::Launcher).unwrap(),
            r#""launcher""#
        );
    }

    #[test]
    fn snapshot_roundtrip_with_operation_and_disabled() {
        let mut s = AppSnapshot::mock_idle();
        s.operation = Some(OperationSnapshot {
            operation_id: 3,
            kind: OperationKind::InstallNode,
            status: OperationStatus::Running,
            stage: "下载中…".into(),
            progress: None,
            error: None,
            started_at: None,
            finished_at: None,
            cancellable: true,
        });
        s.disabled_actions = vec![DisabledAction {
            action: "start".into(),
            reason: "正在安装 Node".into(),
        }];
        let j = serde_json::to_string(&s).unwrap();
        let back: AppSnapshot = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        assert!(j.contains("\"operationId\":3"), "{j}");
    }

    #[test]
    fn tool_runtime_serde_camel_and_lowercase() {
        let t = ToolRuntime {
            version: Some("v24.9.0".into()),
            source: Some(ToolSource::Managed),
            path: Some("/tmp/toolchains/node/v24.9.0/bin/node".into()),
            status: ToolCheck::Detected,
            verified: true,
            hint: None,
            managed_available: true,
        };
        let j = serde_json::to_string(&t).unwrap();
        assert!(j.contains("\"version\":\"v24.9.0\""), "{j}");
        assert!(j.contains("\"source\":\"managed\""), "{j}");
        assert!(j.contains("\"status\":\"detected\""), "{j}");
        assert!(j.contains("\"managedAvailable\":true"), "{j}");
        let back: ToolRuntime = serde_json::from_str(&j).unwrap();
        assert_eq!(back, t);

        // 系统工具:verified 恒 false,来源 system
        let sys = ToolRuntime {
            version: Some("2.47.0".into()),
            source: Some(ToolSource::System),
            path: Some("/usr/bin/git".into()),
            status: ToolCheck::Detected,
            verified: false,
            hint: None,
            managed_available: false,
        };
        let js = serde_json::to_string(&sys).unwrap();
        assert!(js.contains("\"source\":\"system\""), "{js}");
        assert!(js.contains("\"verified\":false"), "{js}");
        assert!(js.contains("\"managedAvailable\":false"), "{js}");
        let back: ToolRuntime = serde_json::from_str(&js).unwrap();
        assert_eq!(back, sys);

        assert!(!sys.is_missing());
        let missing = ToolRuntime {
            status: ToolCheck::Missing,
            ..sys
        };
        assert!(missing.is_missing());
    }
}
