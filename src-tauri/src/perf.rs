// dsh-launcher · 启动/运行性能测量点
//
// 记录相对进程启动(process_start)的命名时间戳,并广播 app://perf-metrics 事件。
// 测量点:
//   process_start     进程入口(main 首行)
//   tauri_ready       Tauri setup 完成
//   main_window_visible 主窗口首次可见
//   env_check_done    环境检查完成
//   repo_check_done   仓库状态扫描完成
//   dsh_ready         dsh web 就绪
//   chat_load_finished chat WebView 页面加载完成
//   react_interactive React 首帧可交互(renderer 通过 perf_mark 上报)
//
// 前端可用 P50/P95 口径聚合;本模块只负责记录与广播,不做主观优化决策。
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PerfMark {
    pub name: String,
    /// 相对 process_start 的毫秒数。
    pub ms: i64,
}

pub const EVENT_PERF_METRICS: &str = "app://perf-metrics";

pub struct BootTimings {
    /// 进程启动瞬间(静态,由 main() 首行捕获)。
    start: Instant,
    marks: Mutex<Vec<PerfMark>>,
}

impl BootTimings {
    pub fn new(start: Instant) -> Self {
        Self {
            start,
            marks: Mutex::new(Vec::new()),
        }
    }

    /// 记录一个测量点(同名覆盖,取最后一次)。
    pub fn mark(&self, name: &str) -> i64 {
        let ms = self.start.elapsed().as_millis() as i64;
        let mut marks = self.marks.lock().unwrap();
        if let Some(existing) = marks.iter_mut().find(|m| m.name == name) {
            existing.ms = ms;
        } else {
            marks.push(PerfMark {
                name: name.to_string(),
                ms,
            });
        }
        ms
    }

    pub fn snapshot(&self) -> Vec<PerfMark> {
        self.marks.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_are_monotonic_and_dedupe() {
        let t = BootTimings::new(Instant::now());
        let a = t.mark("react_interactive");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let b = t.mark("env_check_done");
        assert!(b >= a);
        let c = t.mark("react_interactive");
        assert!(c >= b);
        let marks = t.snapshot();
        assert_eq!(marks.len(), 2, "同名应覆盖");
        assert!(marks
            .iter()
            .any(|m| m.name == "react_interactive" && m.ms == c));
    }

    #[test]
    fn serde_camel_case() {
        let m = PerfMark {
            name: "tauri_ready".into(),
            ms: 123,
        };
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(j, r#"{"name":"tauri_ready","ms":123}"#);
    }
}
