// dsh-launcher · LogHub:内存 ring + 文件落盘 + 事件广播 + 脱敏
// ring 上限 2,000 条(长期后台运行内存不增长);日志写失败不阻塞主流程。
use crate::contract::{LogEntry, LogLevel, LogPage};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const RING_CAP: usize = 2_000;

/// 日志写入回调(Tauri 事件广播由外层注入;测试可注入 no-op)。
pub type LogSink = dyn Fn(&LogEntry) + Send + Sync + 'static;

/// 已知来源(Logs 页筛选)。
pub const SOURCES: [&str; 5] = ["launcher", "dsh web", "dev:web", "git", "pnpm"];

pub struct LogHub {
    ring: Mutex<VecDeque<LogEntry>>,
    next_id: Mutex<u64>,
    file: Mutex<Option<std::fs::File>>,
    file_path: PathBuf,
    sink: Arc<LogSink>,
    redact: bool,
}

impl LogHub {
    pub fn new(file_path: PathBuf, sink: Arc<LogSink>, redact: bool) -> Self {
        Self {
            ring: Mutex::new(VecDeque::new()),
            next_id: Mutex::new(1),
            file: Mutex::new(None),
            file_path,
            sink,
            redact,
        }
    }

    /// 脱敏:URL query、Authorization、Bearer token、已知密钥格式。
    pub fn redact_text(text: &str) -> String {
        let mut out = text.to_string();
        // URL query:?token=xxx / &token=xxx
        let keep = [
            "token",
            "key",
            "secret",
            "password",
            "auth",
            "sign",
            "sig",
            "access_token",
        ];
        for k in keep {
            let pats = [format!("{k}="), format!("{k}%3D")];
            for p in pats {
                let mut rest = out.as_str();
                let mut buf = String::new();
                while let Some(idx) = rest.to_lowercase().find(&p.to_lowercase()) {
                    buf.push_str(&rest[..idx + p.len()]);
                    let tail = &rest[idx + p.len()..];
                    let take = tail
                        .chars()
                        .take_while(|c| !c.is_whitespace() && *c != '&' && *c != '#' && *c != '"')
                        .count();
                    let len: usize = tail.chars().take(take).map(char::len_utf8).sum();
                    buf.push_str("[redacted]");
                    rest = &tail[len..];
                }
                buf.push_str(rest);
                out = buf;
            }
        }
        // 裸 Authorization 头:替换 "Bearer <值>" 整段
        const AUTH_MARKER: &str = "Authorization: Bearer ";
        if let Some(idx) = out.find(AUTH_MARKER) {
            let after = &out[idx + AUTH_MARKER.len()..];
            let take = after
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
                .count();
            let take_len: usize = after.chars().take(take).map(char::len_utf8).sum();
            if take_len > 0 {
                out.replace_range(
                    idx + AUTH_MARKER.len()..idx + AUTH_MARKER.len() + take_len,
                    "[redacted]",
                );
            }
        }
        out
    }

    /// 追加一条日志:ring + 文件 + 广播。
    pub fn append(&self, src: &str, level: LogLevel, text: &str) {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let id = *n;
            *n = n.wrapping_add(1);
            id
        };
        let entry = LogEntry {
            id,
            ts: chrono_now_ms(),
            src: src.to_string(),
            level,
            text: if self.redact {
                Self::redact_text(text)
            } else {
                text.to_string()
            },
        };
        {
            let mut ring = self.ring.lock().unwrap();
            ring.push_back(entry.clone());
            while ring.len() > RING_CAP {
                ring.pop_front();
            }
        }
        self.write_file(&entry);
        (self.sink)(&entry);
    }

    fn write_file(&self, entry: &LogEntry) {
        let mut f = self.file.lock().unwrap();
        if f.is_none() {
            if let Some(parent) = self.file_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            *f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
                .ok();
        }
        if let Some(file) = f.as_mut() {
            let line = format!(
                "{} [{}] {} {}\n",
                entry.ts,
                entry.src,
                format!("{:?}", entry.level).to_lowercase(),
                entry.text
            );
            if file.write_all(line.as_bytes()).is_err() {
                *f = None; // 写失败:下次重开,不阻塞主流程
            }
        }
    }

    /// id > since 的增量(Logs 页首次进入拉历史)。
    pub fn snapshot(&self, since_id: u64) -> LogPage {
        let ring = self.ring.lock().unwrap();
        LogPage {
            logs: ring.iter().filter(|l| l.id > since_id).cloned().collect(),
            sources: SOURCES.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn clear(&self) {
        self.ring.lock().unwrap().clear();
    }
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_hub() -> LogHub {
        LogHub::new(
            std::env::temp_dir().join(format!("dsh-loghub-test-{}.log", std::process::id())),
            Arc::new(|_| {}),
            true,
        )
    }

    #[test]
    fn ring_caps_at_2000() {
        let hub = noop_hub();
        for i in 0..2_500 {
            hub.append("launcher", LogLevel::Info, &format!("line {i}"));
        }
        let page = hub.snapshot(0);
        assert!(page.logs.len() <= RING_CAP, "ring 应限制在 {RING_CAP}");
        assert_eq!(page.logs.len(), RING_CAP);
        assert_eq!(page.logs.first().unwrap().text, "line 500");
        assert_eq!(page.logs.last().unwrap().text, "line 2499");
    }

    #[test]
    fn snapshot_incremental() {
        let hub = noop_hub();
        for i in 0..5 {
            hub.append("launcher", LogLevel::Ok, &format!("e{i}"));
        }
        let page = hub.snapshot(3);
        assert_eq!(page.logs.len(), 2);
        assert_eq!(page.logs[0].id, 4);
        assert_eq!(page.sources.len(), SOURCES.len());
    }

    #[test]
    fn clear_empties_ring() {
        let hub = noop_hub();
        hub.append("launcher", LogLevel::Info, "x");
        hub.clear();
        assert!(hub.snapshot(0).logs.is_empty());
    }

    #[test]
    fn redact_urls_and_tokens() {
        let t = LogHub::redact_text("GET http://127.0.0.1:3090/api/logs?token=abc123def token=b1b");
        assert!(!t.contains("abc123def"), "query token 应被脱敏: {t}");
        assert!(t.contains("[redacted]"));
        let a = LogHub::redact_text("Authorization: Bearer xyzsecret");
        assert!(!a.contains("xyzsecret"));
        let plain = LogHub::redact_text("normal log line");
        assert_eq!(plain, "normal log line");
    }

    #[test]
    fn file_write_happens() {
        let path = std::env::temp_dir().join(format!("dsh-loghub-file-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let hub = LogHub::new(path.clone(), Arc::new(|_| {}), true);
        hub.append("git", LogLevel::Info, "hello log");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello log"));
        assert!(content.contains("git"));
        let _ = std::fs::remove_file(&path);
    }
}
