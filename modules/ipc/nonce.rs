// AGPL-3.0 许可证
// nonce 文件链（红线 1 / §3.9）：GUID 命名互不踩踏；写前 DACL 断继承收紧（owner+SYSTEM[+克隆]）；
// 独占 CREATE_NEW 写；接收方读后即删（一次性消费）；残留可 sweep。nonce 本体永不进 argv。

use std::fs::{self, File};
use std::io::{Error, ErrorKind, Result, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use rand::RngCore;
use tracing::{error, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_FLAG_WRITE_THROUGH, FILE_SHARE_MODE,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;

pub use crate::ipc::frame::NONCE_LEN;

/// %PROGRAMDATA%\AkiSpace——目录不存在则拒操，由写方 create_dir_all
pub fn nonce_dir() -> PathBuf {
    let root = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    root.join("AkiSpace")
}

/// nonce_<Guid:N>.bin——每次会话随机 GUID，并发握手互不踩踏（红线 1）
pub fn new_nonce_path() -> PathBuf {
    nonce_dir().join(format!("nonce_{}.bin", uuid::Uuid::new_v4().as_simple()))
}

pub fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// 收紧 DACL（断继承，仅白名单 GA）：owner 当前用户 + SYSTEM，另可附克隆账户 SID（读后即删需要 DELETE）。
/// 与管道建流共用 acl::build_pipe_dacl_sd。
pub fn tight_file_dacl(extra_sids: &[Vec<u8>]) -> Result<crate::ipc::acl::SddlSecurity> {
    let mut sids = vec![crate::ipc::acl::current_user_sid()?, crate::ipc::acl::system_sid()?];
    sids.extend_from_slice(extra_sids);
    crate::ipc::acl::build_pipe_dacl_sd(&sids)
}

/// 独占写 nonce 文件：CREATE_NEW + FileShare.None（TOCTOU/双写守卫）+ 写透。
/// DACL 收紧失败 → error! 记录后回退令牌默认 DACL（不静默；默认 DACL 仍仅 owner/SYSTEM）。
pub fn write_nonce_file(path: &Path, nonce: &[u8; NONCE_LEN], extra_sids: &[Vec<u8>]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::new(e.kind(), format!("创建 nonce 目录失败 {parent:?}: {e}")))?;
    }
    let wide = wide_path(path);
    let mut sec = match tight_file_dacl(extra_sids) {
        Ok(s) => Some(s),
        Err(e) => {
            error!("nonce DACL 收紧失败，回退令牌默认 DACL（不静默）: {e}");
            None
        }
    };
    let sa_ptr: *mut SECURITY_ATTRIBUTES = match &mut sec {
        Some(s) => s.raw() as *mut SECURITY_ATTRIBUTES,
        None => std::ptr::null_mut(),
    };
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            GENERIC_WRITE.0,
            // FileShare 为 0：独占，别的句柄连开都开不了
            FILE_SHARE_MODE(0),
            if sa_ptr.is_null() { None } else { Some(&*sa_ptr) },
            CREATE_NEW,
            FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0 | FILE_FLAG_WRITE_THROUGH.0),
            None,
        )
    }
    .map_err(|e| Error::new(ErrorKind::Other, format!("nonce 文件独占创建失败 {path:?}: {e}")))?;
    let mut file = unsafe { File::from_raw_handle(handle.0 as _) };
    let written = file.write_all(nonce).and_then(|_| file.flush());
    drop(file);
    written
}

/// 目录或文件路径皆可：给目录则取其中最新的 nonce_*.bin（agent 端不知道 GUID 文件名）
pub fn resolve_nonce_target(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        find_latest_nonce_file(path)
    } else {
        Some(path.to_path_buf())
    }
}

fn is_nonce_file_name(name: &str) -> bool {
    name.starts_with("nonce_") && name.ends_with(".bin")
}

pub fn find_latest_nonce_file(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        if !is_nonce_file_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let Ok(m) = entry.metadata() else { continue };
        let Ok(t) = m.modified() else { continue };
        if best.as_ref().map_or(true, |(bt, _)| t > *bt) {
            best = Some((t, entry.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// 写新 nonce 前清残留：死掉的旧会话留下的尸体既泄元数据也干扰对账（红线 1）
pub fn sweep_stale_nonces(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if is_nonce_file_name(&entry.file_name().to_string_lossy()) {
            let p = entry.path();
            if let Err(e) = fs::remove_file(&p) {
                warn!("清理残留 nonce 失败 {p:?}: {e}");
            }
        }
    }
}

/// 接收方一次性消费：读出 32 字节并删除文件——读失败也删（尸体不复活，语义对齐 §3.9 finally Delete）
pub fn read_and_delete_nonce(path: &Path) -> Result<[u8; NONCE_LEN]> {
    let target = resolve_nonce_target(path)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("{path:?} 内无 nonce 文件")))?;
    let bytes = fs::read(&target);
    match fs::remove_file(&target) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => warn!("nonce 删除失败 {target:?}: {e}"),
    }
    let bytes = bytes?;
    if bytes.len() != NONCE_LEN {
        return Err(Error::new(ErrorKind::InvalidData, "Nonce 长度错误"));
    }
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&bytes);
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_and_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonce_roundtrip.bin");
        let nonce = generate_nonce();
        write_nonce_file(&path, &nonce, &[]).unwrap();
        assert_eq!(read_and_delete_nonce(&path).unwrap(), nonce);
        assert!(!path.exists(), "读后必须删（一次性消费）");
        assert!(read_and_delete_nonce(&path).is_err(), "二次消费必须失败");
    }

    #[test]
    fn exclusive_create_rejects_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonce_excl.bin");
        write_nonce_file(&path, &[1u8; NONCE_LEN], &[]).unwrap();
        assert!(write_nonce_file(&path, &[2u8; NONCE_LEN], &[]).is_err(), "CREATE_NEW 撞车必须失败");
    }

    #[test]
    fn wrong_length_nonce_is_consumed_and_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonce_short.bin");
        fs::write(&path, [0u8; 5]).unwrap();
        assert!(read_and_delete_nonce(&path).is_err());
        assert!(!path.exists(), "非法长度同样读后即删");
    }

    #[test]
    fn dir_target_resolves_latest_nonce_file() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join(format!("nonce_{}.bin", uuid::Uuid::new_v4().as_simple()));
        write_nonce_file(&keep, &[3u8; NONCE_LEN], &[]).unwrap();
        fs::write(dir.path().join("unrelated.txt"), b"x").unwrap();
        fs::write(dir.path().join("other.bin"), b"x").unwrap();
        let got = read_and_delete_nonce(dir.path()).unwrap();
        assert_eq!(got, [3u8; NONCE_LEN]);
        assert!(!keep.exists());
    }

    #[test]
    fn sweep_removes_only_nonce_files() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("nonce_stale.bin");
        fs::write(&stale, [0u8; NONCE_LEN]).unwrap();
        fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        sweep_stale_nonces(dir.path());
        assert!(!stale.exists());
        assert!(dir.path().join("settings.json").exists());
    }

    #[test]
    fn nonce_path_carries_guid_filename() {
        let p = new_nonce_path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("nonce_") && name.ends_with(".bin"), "{name}");
        assert_eq!(name.len(), "nonce_".len() + ".bin".len() + 32, "Guid:N 简写 32 位十六进制");
    }
}
