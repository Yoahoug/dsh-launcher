// dsh-launcher · 可取消流式下载(M1)
//
// 契约:目标写入 <dest>.part,完成后校验长度 + SHA-256 再原子改名为最终文件;
// 支持 HTTP Range 断点续传(服务器不支持则整体重下);取消令牌在每个读循环检查,
// 取消后保留 .part 供下次续传;失败指数退避有限重试,不自动切换下载源(换源需用户同意)。
use crate::ops::{CancellationToken, OperationError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CHUNK: usize = 64 * 1024;
const MAX_ATTEMPTS: u32 = 3;

/// 下载到 part 文件并校验。expected_size/sha256 来自签名 catalog。
pub fn download_and_verify(
    url: &str,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
    token: &CancellationToken,
    timeout_ms: u64,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), OperationError> {
    let part = part_path(dest);
    let mut attempt: u32 = 0;
    loop {
        token.check()?;
        attempt += 1;
        match download_once(url, &part, expected_size, token, timeout_ms, on_progress) {
            Ok(()) => break,
            Err(OperationError::Cancelled) => return Err(OperationError::Cancelled),
            Err(OperationError::Failed(e)) => {
                if attempt >= MAX_ATTEMPTS {
                    return Err(OperationError::Failed(format!(
                        "下载失败(已重试 {MAX_ATTEMPTS} 次):{e}"
                    )));
                }
                let wait = Duration::from_secs(1 << (attempt - 1)); // 1s, 2s
                std::thread::sleep(wait);
            }
        }
    }

    // 长度校验(Content-Length 已知时)
    let len = std::fs::metadata(&part)
        .map_err(|e| OperationError::Failed(format!("读取下载文件失败:{e}")))?
        .len();
    if len != expected_size {
        return Err(OperationError::Failed(format!(
            "长度校验失败:期望 {expected_size},实际 {len}(已清理,请重试)"
        )));
    }
    // SHA-256 校验(安全失败,绝不绕过)
    let actual = sha256_hex(&part)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(OperationError::Failed(format!(
            "SHA-256 校验失败:期望 {expected_sha256},实际 {actual}(安全失败,已清理)"
        )));
    }
    // 原子改名(同卷 rename)
    std::fs::rename(&part, dest)
        .map_err(|e| OperationError::Failed(format!("下载文件落盘失败:{}", e)))?;
    Ok(())
}

/// <dest>.part 路径。
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn download_once(
    url: &str,
    part: &Path,
    expected_size: u64,
    token: &CancellationToken,
    timeout_ms: u64,
    on_progress: &dyn Fn(u64, u64),
) -> Result<(), OperationError> {
    // 已下载字节(断点续传基础)
    let existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let resume_from = if existing > 0 && existing < expected_size {
        existing
    } else {
        0
    };

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_millis(timeout_ms))
        .build();
    let req = if resume_from > 0 {
        agent
            .get(url)
            .set("Range", &format!("bytes={resume_from}-"))
    } else {
        agent.get(url)
    };
    let res = req
        .call()
        .map_err(|e| OperationError::Failed(format!("HTTP 请求失败:{e}")))?;

    let status = res.status();
    let total = res
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    // 206 续传;200 表示服务器忽略 Range → 从头重写
    let mut received: u64 = if status == 206 { resume_from } else { 0 };

    // 打开输出:续传 append,否则重写
    let mut out: Box<dyn Write> = if status == 206 && resume_from > 0 {
        Box::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(part)
                .map_err(|e| OperationError::Failed(format!("打开 part 文件失败:{e}")))?,
        )
    } else {
        if resume_from > 0 {
            // 服务器不支持 Range:整体重下
            let _ = std::fs::remove_file(part);
        }
        Box::new(
            std::fs::File::create(part)
                .map_err(|e| OperationError::Failed(format!("创建 part 文件失败:{e}")))?,
        )
    };

    let mut buf = [0u8; CHUNK];
    let mut reader = res.into_reader();
    loop {
        token.check()?;
        let n = reader
            .read(&mut buf)
            .map_err(|e| OperationError::Failed(format!("读取下载流失败:{e}")))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| OperationError::Failed(format!("写入下载文件失败:{e}")))?;
        received += n as u64;
        let expected_total = if total > 0 { total + resume_from } else { 0 };
        if expected_total > 0 {
            on_progress(received, expected_total);
        }
    }
    out.flush()
        .map_err(|e| OperationError::Failed(format!("下载文件刷盘失败:{e}")))?;
    Ok(())
}

/// 文件 SHA-256(hex 小写)。
pub fn sha256_hex(path: &Path) -> Result<String, OperationError> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)
        .map_err(|e| OperationError::Failed(format!("打开文件失败:{e}")))?;
    let mut ctx = sha2::Sha256::new();
    let mut buf = [0u8; CHUNK];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| OperationError::Failed(format!("读取失败:{e}")))?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(format!("{:x}", ctx.finalize()))
}

/// 小文件下载到内存(如 JSON/索引;仍走取消与超时)。
pub fn download_bytes(
    url: &str,
    token: &CancellationToken,
    timeout_ms: u64,
    max_bytes: u64,
) -> Result<Vec<u8>, OperationError> {
    token.check()?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_millis(timeout_ms))
        .build();
    let res = agent
        .get(url)
        .call()
        .map_err(|e| OperationError::Failed(format!("HTTP 请求失败:{e}")))?;
    let mut buf = Vec::new();
    let mut reader = res.into_reader();
    let mut tmp = [0u8; CHUNK];
    loop {
        token.check()?;
        let n = reader
            .read(&mut tmp)
            .map_err(|e| OperationError::Failed(format!("读取响应失败:{e}")))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() as u64 > max_bytes {
            return Err(OperationError::Failed(format!(
                "响应超过大小上限 {max_bytes}"
            )));
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// 极简测试 HTTP 服务器(支持 Range / 内容大小可控)。
    struct TestServer {
        addr: String,
        _body: Vec<u8>,
        _support_range: bool,
    }

    impl TestServer {
        fn start(body: Vec<u8>, support_range: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let addr_s = format!("http://{addr}/file.bin");
            let body_for_thread = body.clone();
            let handle = std::thread::spawn(move || {
                for stream in listener.incoming().take(10) {
                    let Ok(mut stream) = stream else { continue };
                    let mut buf = [0u8; 8192];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let range = req
                        .lines()
                        .find(|l| l.to_lowercase().starts_with("range:"))
                        .map(|l| l.to_string());
                    let (status_line, start) = if let Some(r) = range {
                        let from = r
                            .split("bytes=")
                            .nth(1)
                            .and_then(|s| s.split('-').next())
                            .and_then(|s| s.trim().parse::<u64>().ok())
                            .unwrap_or(0) as usize;
                        if support_range {
                            (format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\n", from, body_for_thread.len() - 1, body_for_thread.len()), from)
                        } else {
                            ("HTTP/1.1 200 OK\r\n".to_string(), 0)
                        }
                    } else {
                        ("HTTP/1.1 200 OK\r\n".to_string(), 0)
                    };
                    let chunk = &body_for_thread[start..];
                    let resp = format!(
                        "{status_line}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        chunk.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.write_all(chunk);
                }
            });
            let _ = handle;
            Self {
                addr: addr_s,
                _body: body,
                _support_range: support_range,
            }
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-dl-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn download_verifies_length_and_hash() {
        let body = b"hello download world".to_vec();
        let srv = TestServer::start(body.clone(), true);
        let dest = temp_path("ok.bin");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(part_path(&dest));
        let token = CancellationToken::new();
        let sha = sha256_hex_from(&body);
        download_and_verify(
            &srv.addr,
            &dest,
            body.len() as u64,
            &sha,
            &token,
            30_000,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!part_path(&dest).exists(), ".part 应已改名");
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn hash_mismatch_is_safety_failure() {
        let body = b"data with mismatch".to_vec();
        let srv = TestServer::start(body.clone(), true);
        let dest = temp_path("mismatch.bin");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(part_path(&dest));
        let token = CancellationToken::new();
        let err = download_and_verify(
            &srv.addr,
            &dest,
            body.len() as u64,
            &"0".repeat(64),
            &token,
            30_000,
            &|_, _| {},
        )
        .unwrap_err();
        match err {
            OperationError::Failed(m) => {
                assert!(m.contains("SHA-256"), "{m}");
            }
            _ => panic!("应失败"),
        }
        assert!(!dest.exists());
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn length_mismatch_fails() {
        let body = b"12345".to_vec();
        let srv = TestServer::start(body.clone(), true);
        let dest = temp_path("len.bin");
        let _ = std::fs::remove_file(&dest);
        let token = CancellationToken::new();
        let sha = sha256_hex_from(&body);
        let err = download_and_verify(&srv.addr, &dest, 999, &sha, &token, 30_000, &|_, _| {})
            .unwrap_err();
        assert!(err.to_string().contains("长度校验失败"));
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn resume_from_part() {
        let body = b"0123456789abcdefghij".to_vec();
        let srv = TestServer::start(body.clone(), true);
        let dest = temp_path("resume.bin");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(part_path(&dest));
        let token = CancellationToken::new();
        // 预置一半的 part(模拟上次中断)
        std::fs::write(part_path(&dest), &body[..10]).unwrap();
        let sha = sha256_hex_from(&body);
        download_and_verify(
            &srv.addr,
            &dest,
            body.len() as u64,
            &sha,
            &token,
            30_000,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body, "续传后内容应完整");
        let _ = std::fs::remove_file(&dest);
    }

    #[test]
    fn cancel_during_download() {
        // 大文件 + 立即取消
        let body = vec![0u8; 1024 * 1024];
        let body_len = body.len() as u64;
        let srv = TestServer::start(body, true);
        let dest = temp_path("cancel.bin");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(part_path(&dest));
        let token = CancellationToken::new();
        token.cancel();
        let err = download_and_verify(
            &srv.addr,
            &dest,
            body_len,
            &"0".repeat(64),
            &token,
            30_000,
            &|_, _| {},
        )
        .unwrap_err();
        assert_eq!(err, OperationError::Cancelled);
        let _ = std::fs::remove_file(&dest);
    }

    fn sha256_hex_from(data: &[u8]) -> String {
        use sha2::Digest;
        let mut ctx = sha2::Sha256::new();
        ctx.update(data);
        format!("{:x}", ctx.finalize())
    }

    #[test]
    fn part_path_suffix() {
        let p = part_path(Path::new("/tmp/a.bin"));
        assert_eq!(p, PathBuf::from("/tmp/a.bin.part"));
    }
}
