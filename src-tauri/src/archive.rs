// dsh-launcher · 安全解压(M1)
//
// 防 Zip Slip:拒绝绝对路径、盘符、`..` 组件、符号链接/硬链接/junction;
// 限制条目数与解压后总体积;取消令牌逐条目检查。
use crate::ops::{CancellationToken, OperationError};
use std::path::{Component, Path, PathBuf};

/// 解压安全上限。
pub const MAX_ENTRIES: u64 = 200_000;
pub const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024; // 4 GiB

/// 校验归档内条目路径是否安全(不落盘)。
fn check_entry_name(name: &str) -> Result<(), OperationError> {
    let normalized = name.replace('\\', "/");
    let p = Path::new(&normalized);
    // 绝对路径 / 盘符
    if p.is_absolute() || normalized.starts_with('/') || normalized.contains(':') {
        return Err(OperationError::Failed(format!(
            "归档条目包含绝对路径/盘符,拒绝: {name}"
        )));
    }
    // .. 组件
    for c in p.components() {
        if let Component::ParentDir = c {
            return Err(OperationError::Failed(format!(
                "归档条目包含 .. 路径穿越,拒绝: {name}"
            )));
        }
    }
    if normalized.is_empty() {
        return Err(OperationError::Failed("归档条目名为空".into()));
    }
    Ok(())
}

/// 安全 join:先校验组件,再拼接(双保险)。
fn safe_join(dest: &Path, name: &str) -> Result<PathBuf, OperationError> {
    check_entry_name(name)?;
    let out = dest.join(name.replace('\\', "/"));
    // 确保解析后仍在 dest 内(防御连接点/软链已存在导致的逃逸)
    if !out.starts_with(dest) {
        return Err(OperationError::Failed(format!(
            "条目逃逸目标目录,拒绝: {name}"
        )));
    }
    Ok(out)
}

/// 词法规范化路径(折叠 `.`/`..`,不访问文件系统;用于符号链接解析判断)。
fn normalize_lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 创建安全符号链接(unix;Windows 上 Node 官方 tar 不使用,MinGit zip 无链接)。
#[cfg(unix)]
fn create_symlink(out: &Path, target: &Path) -> Result<(), OperationError> {
    std::os::unix::fs::symlink(target, out)
        .map_err(|e| OperationError::Failed(format!("创建符号链接失败:{e}")))
}

#[cfg(windows)]
fn create_symlink(_out: &Path, _target: &Path) -> Result<(), OperationError> {
    Err(OperationError::Failed(
        "Windows 上不允许 tar 符号链接(需管理员权限)".into(),
    ))
}

/// 安全解压 zip。
pub fn extract_zip(
    zip_path: &Path,
    dest: &Path,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| OperationError::Failed(format!("打开 zip 失败:{e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| OperationError::Failed(format!("读取 zip 失败:{e}")))?;
    if archive.len() as u64 > MAX_ENTRIES {
        return Err(OperationError::Failed(format!(
            "zip 条目数超限:{}",
            archive.len()
        )));
    }
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        token.check()?;
        let mut entry = archive
            .by_index(i)
            .map_err(|e| OperationError::Failed(format!("读取 zip 条目失败:{e}")))?;
        let raw_name = entry.name().to_string();
        // 拒绝符号链接/硬链接(junction 在 Windows 上由 zip 模拟为符号链接)
        #[cfg(unix)]
        {
            let mode = entry.unix_mode().unwrap_or(0);
            if mode & 0o170000 == 0o120000 {
                return Err(OperationError::Failed(format!(
                    "zip 包含符号链接,拒绝: {raw_name}"
                )));
            }
        }
        let name = raw_name.trim_end_matches('/');
        if name.is_empty() {
            continue; // 根目录条目
        }
        let out = safe_join(dest, name)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out)
                .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
        }
        // 解压体积上限(解压前已知,不必全量写入)
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| OperationError::Failed("解压总体积溢出".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(OperationError::Failed(format!(
                "解压总体积超限(>{MAX_TOTAL_BYTES} 字节),拒绝"
            )));
        }
        let mut out_file = std::fs::File::create(&out)
            .map_err(|e| OperationError::Failed(format!("创建文件失败:{e}")))?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|e| OperationError::Failed(format!("解压条目失败:{e}")))?;
    }
    Ok(())
}

/// 安全解压 tar.gz(Node 官方 tar.gz)。
pub fn extract_tar_gz(
    tar_path: &Path,
    dest: &Path,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let file = std::fs::File::open(tar_path)
        .map_err(|e| OperationError::Failed(format!("打开 tar.gz 失败:{e}")))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(true);
    let entries = archive
        .entries()
        .map_err(|e| OperationError::Failed(format!("读取 tar 失败:{e}")))?;
    let mut count: u64 = 0;
    let mut total: u64 = 0;
    // 链接最后创建，避免后续归档条目把它当作父目录并经链接写入非预期位置。
    let mut pending_symlinks: Vec<(PathBuf, PathBuf)> = Vec::new();
    for entry in entries {
        token.check()?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(OperationError::Failed(format!(
                "tar 条目数超限(>{MAX_ENTRIES})"
            )));
        }
        let mut entry = entry.map_err(|e| OperationError::Failed(format!("tar 条目失败:{e}")))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() {
            // 仅允许「相对且不逃逸解压根」的符号链接(Node 官方 tar 的 bin/npm → ../lib/…)
            let target = entry
                .header()
                .link_name()
                .map_err(|e| OperationError::Failed(format!("tar 链接目标非法:{e}")))?
                .ok_or_else(|| OperationError::Failed("tar 符号链接缺少目标".into()))?;
            let raw_target = target.to_string_lossy().to_string();
            let normalized_target = raw_target.replace('\\', "/");
            if normalized_target.starts_with('/') || raw_target.contains(':') {
                return Err(OperationError::Failed(format!(
                    "tar 符号链接目标为绝对路径/盘符,拒绝: {raw_target}"
                )));
            }
            let raw_path = entry
                .path()
                .map_err(|e| OperationError::Failed(format!("tar 路径非法:{e}")))?
                .to_string_lossy()
                .to_string();
            let out = safe_join(dest, &raw_path)?;
            // 关键:链接目标按「链接所在目录」解析后必须仍在解压根内
            // (允许 ../lib 这种安全的相对链接,拒绝 ../../ 逃逸)
            let link_dir = out.parent().unwrap_or(dest);
            let resolved = normalize_lexical(&link_dir.join(&normalized_target));
            if !resolved.starts_with(dest) {
                return Err(OperationError::Failed(format!(
                    "tar 符号链接目标逃逸解压根,拒绝: {raw_target}"
                )));
            }
            pending_symlinks.push((out, target.into_owned()));
            continue;
        }
        if entry_type.is_hard_link() {
            return Err(OperationError::Failed("tar 包含硬链接,拒绝".into()));
        }
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(OperationError::Failed(format!(
                "tar 包含不支持的条目类型,拒绝:{entry_type:?}"
            )));
        }
        let raw = entry
            .path()
            .map_err(|e| OperationError::Failed(format!("tar 路径非法:{e}")))?
            .to_string_lossy()
            .to_string();
        let out = safe_join(dest, &raw)?;
        if entry_type.is_dir() {
            std::fs::create_dir_all(&out)
                .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
        }
        let size = entry.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| OperationError::Failed("解压总体积溢出".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(OperationError::Failed(format!(
                "解压总体积超限(>{MAX_TOTAL_BYTES} 字节),拒绝"
            )));
        }
        entry
            .unpack(&out)
            .map_err(|e| OperationError::Failed(format!("解压条目失败:{e}")))?;
    }
    for (out, target) in pending_symlinks {
        token.check()?;
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| OperationError::Failed(format!("创建目录失败:{e}")))?;
        }
        create_symlink(&out, &target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let base =
            std::env::temp_dir().join(format!("dsh-arc-test-{}-{name}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn build_zip_with(entries: &[(&str, &[u8])]) -> (PathBuf, PathBuf) {
        // 用 zip 库写一个测试 zip
        let base = temp_dir("build");
        let zip_path = base.join("test.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        (zip_path, base)
    }

    #[test]
    fn extract_normal_zip() {
        let (zip_path, base) = build_zip_with(&[("a/b.txt", b"hello"), ("c.txt", b"world")]);
        let dest = temp_dir("out");
        extract_zip(&zip_path, &dest, &CancellationToken::new()).unwrap();
        assert_eq!(std::fs::read(dest.join("a/b.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(dest.join("c.txt")).unwrap(), b"world");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn zip_slip_dotdot_rejected() {
        let (zip_path, base) = build_zip_with(&[("../evil.txt", b"x")]);
        let dest = temp_dir("out2");
        let err = extract_zip(&zip_path, &dest, &CancellationToken::new()).unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
        assert!(!base.join("evil.txt").exists() && !dest.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn zip_absolute_and_drive_rejected() {
        for name in ["/abs/path.txt", "C:/win.txt"] {
            let (zip_path, base) = build_zip_with(&[(name, b"x")]);
            let dest = temp_dir("out3");
            let err = extract_zip(&zip_path, &dest, &CancellationToken::new()).unwrap_err();
            assert!(err.to_string().contains("拒绝"), "{name} → {err}");
            let _ = std::fs::remove_dir_all(&base);
            let _ = std::fs::remove_dir_all(&dest);
        }
    }

    #[test]
    fn tar_gz_normal_and_slip() {
        // 正常 tar.gz
        let base = temp_dir("tar");
        let tar_path = base.join("node.tar.gz");
        {
            let file = std::fs::File::create(&tar_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut w = tar::Builder::new(enc);
            let mut h = tar::Header::new_gnu();
            h.set_size(5);
            h.set_mode(0o644);
            h.set_cksum();
            w.append_data(&mut h, "node-v24/x/bin/node", b"node!".as_slice())
                .unwrap();
            w.finish().unwrap();
        }
        let dest = temp_dir("tar-out");
        extract_tar_gz(&tar_path, &dest, &CancellationToken::new()).unwrap();
        assert_eq!(
            std::fs::read(dest.join("node-v24/x/bin/node")).unwrap(),
            b"node!"
        );

        // 路径穿越防护:直接校验 safe_join 拒绝 `..` / 绝对路径
        // (tar::Builder 本身也拒绝构造含 .. 的条目,这里覆盖我们的安全守卫)
        assert!(safe_join(&dest, "../evil.txt").is_err());
        assert!(safe_join(&dest, "/abs/evil.txt").is_err());
        assert!(safe_join(&dest, "C:/evil.txt").is_err());
        assert!(safe_join(&dest, "a/b/c.txt").is_ok());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn tar_safe_symlink_allowed_escaping_rejected() {
        #[cfg(unix)]
        {
            let base = temp_dir("tar-sym");
            let good = base.join("good.tar.gz");
            {
                let file = std::fs::File::create(&good).unwrap();
                let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                let mut w = tar::Builder::new(enc);
                // 安全相对链接:bin/npm -> ../lib/npm-cli.js
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_mode(0o777);
                h.set_cksum();
                w.append_link(&mut h, "root/bin/npm", "../lib/npm-cli.js")
                    .unwrap();
                w.finish().unwrap();
            }
            let dest = temp_dir("tar-sym-out");
            extract_tar_gz(&good, &dest, &CancellationToken::new()).unwrap();
            assert!(
                std::fs::symlink_metadata(dest.join("root/bin/npm"))
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "安全相对符号链接应被创建"
            );

            // 逃逸链接:../../../../escape.txt(root/bin → 越过解压根)
            let evil = base.join("evil.tar.gz");
            {
                let file = std::fs::File::create(&evil).unwrap();
                let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                let mut w = tar::Builder::new(enc);
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_mode(0o777);
                h.set_cksum();
                w.append_link(&mut h, "root/bin/npm", "../../../../escape.txt")
                    .unwrap();
                w.finish().unwrap();
            }
            let dest2 = temp_dir("tar-sym-out2");
            let err = extract_tar_gz(&evil, &dest2, &CancellationToken::new()).unwrap_err();
            assert!(err.to_string().contains("逃逸"), "{err}");
            // 绝对目标链接也应拒绝
            let abs = base.join("abs.tar.gz");
            {
                let file = std::fs::File::create(&abs).unwrap();
                let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                let mut w = tar::Builder::new(enc);
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                h.set_mode(0o777);
                h.set_cksum();
                w.append_link(&mut h, "root/bin/npm", "/etc/passwd")
                    .unwrap();
                w.finish().unwrap();
            }
            let dest3 = temp_dir("tar-sym-out3");
            let err2 = extract_tar_gz(&abs, &dest3, &CancellationToken::new()).unwrap_err();
            assert!(err2.to_string().contains("拒绝"), "{err2}");
            let _ = std::fs::remove_dir_all(&base);
            let _ = std::fs::remove_dir_all(&dest);
            let _ = std::fs::remove_dir_all(&dest2);
        }
    }

    #[test]
    fn tar_rejects_special_entries() {
        let base = temp_dir("tar-special");
        let archive_path = base.join("special.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut writer = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Fifo);
            header.set_size(0);
            header.set_mode(0o644);
            header.set_cksum();
            writer
                .append_data(&mut header, "unexpected-fifo", std::io::empty())
                .unwrap();
            writer.finish().unwrap();
        }
        let dest = temp_dir("tar-special-out");
        let err = extract_tar_gz(&archive_path, &dest, &CancellationToken::new()).unwrap_err();
        assert!(err.to_string().contains("不支持的条目类型"), "{err}");
        assert!(!dest.join("unexpected-fifo").exists());
        let _ = std::fs::remove_dir_all(base);
        let _ = std::fs::remove_dir_all(dest);
    }

    #[test]
    fn cancel_between_entries() {
        let names: Vec<String> = (0..20).map(|i| format!("f{i}.txt")).collect();
        let entries: Vec<(&str, &[u8])> = names
            .iter()
            .map(|n| (n.as_str(), b"x".as_slice()))
            .collect();
        let (zip_path, base) = build_zip_with(&entries);
        let dest = temp_dir("out-cancel");
        let token = CancellationToken::new();
        token.cancel();
        let err = extract_zip(&zip_path, &dest, &token).unwrap_err();
        assert_eq!(err, OperationError::Cancelled);
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&dest);
    }
}
