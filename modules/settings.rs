// AGPL-3.0 许可证

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tracing::warn;
use base64::Engine;

#[cfg(windows)]
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
    CRYPT_INTEGER_BLOB,
};
#[cfg(windows)]
use windows::Win32::Foundation::LocalFree;
#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    StandardRdp,
    ChildSession,
}

impl Default for ConnectionMode {
    fn default() -> Self {
        Self::StandardRdp
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub desktop_width: u32,
    pub desktop_height: u32,
    pub color_depth: u32,
    pub game_mouse_mode_enabled: bool,
    pub audio_redirected: bool,
    pub smart_sizing: bool,
    pub send_system_shortcuts_to_remote: bool,
    pub rdp_port: u16,
    pub auto_connect: bool,
    pub logoff_on_exit: bool,
    pub launch_program_path: Option<String>,
    pub connection_mode: ConnectionMode,
    pub clone_username: String,
    pub clone_password: String,
    pub minimize_to_tray: bool,
    pub show_performance: bool,
    pub enable_global_hotkey: bool,
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            desktop_width: 1920,
            desktop_height: 1080,
            color_depth: 32,
            game_mouse_mode_enabled: false,
            audio_redirected: false,
            smart_sizing: true,
            send_system_shortcuts_to_remote: true,
            rdp_port: 3389,
            auto_connect: false,
            logoff_on_exit: true,
            launch_program_path: None,
            connection_mode: ConnectionMode::default(),
            clone_username: "AkiSpaceUser".into(),
            clone_password: String::new(),
            minimize_to_tray: true,
            show_performance: true,
            enable_global_hotkey: true,
            theme: "dark".into(),
        }
    }
}

impl AppSettings {
    fn settings_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AkiSpace")
            .join("settings.json")
    }

    fn ensure_dir(path: &PathBuf) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    #[cfg(windows)]
    fn dpapi_protect(plaintext: &str) -> String {
        let wide: Vec<u16> = OsStr::new(plaintext).encode_wide().chain(Some(0)).collect();
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: (wide.len() * 2) as u32,
            pbData: wide.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        unsafe {
            // §3.3: 不传 CRYPTPROTECT_LOCAL_MACHINE = CurrentUser 语义，与 C# ProtectedData 互读
            if CryptProtectData(
                &mut in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            ).is_ok()
            {
                let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
                let b64 = base64::engine::general_purpose::STANDARD.encode(slice);
                let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(out_blob.pbData as *mut std::ffi::c_void)));
                return format!("DPAPI:{}", b64);
            }
        }
        plaintext.into()
    }

    #[cfg(windows)]
    fn dpapi_unprotect(protected: &str) -> Option<String> {
        let b64 = protected.strip_prefix("DPAPI:")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        unsafe {
            if CryptUnprotectData(
                &mut in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            ).is_ok()
            {
                let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
                let result = String::from_utf16_lossy(
                    &slice.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()
                );
                let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(out_blob.pbData as *mut std::ffi::c_void)));
                return Some(result);
            }
        }
        None
    }

    #[cfg(not(windows))]
    fn dpapi_protect(plaintext: &str) -> String {
        plaintext.into()
    }

    #[cfg(not(windows))]
    fn dpapi_unprotect(_protected: &str) -> Option<String> {
        None
    }

    fn load_raw() -> Self {
        let path = Self::settings_path();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        let mut settings: Self = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        // §3.3 三态：内存明文 / 磁盘 DPAPI:base64 / 失败 DPAPI:base64(字面量+warn)
        // 升级旧明文密码：无前缀且非空 → 补保护并落盘
        if !settings.clone_password.starts_with("DPAPI:") && !settings.clone_password.is_empty() {
            settings.clone_password = Self::dpapi_protect(&settings.clone_password);
            if let Err(e) = settings.save() {
                warn!("旧密码重加密落盘失败: {}", e);
            }
        }
        settings
    }

    fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::settings_path();
        Self::ensure_dir(&path);
        let json = serde_json::to_string_pretty(self)?;
        let mut tmp = NamedTempFile::new_in(path.parent().unwrap())?;
        std::io::Write::write_all(&mut tmp, json.as_bytes())?;
        tmp.persist(&path)?;
        Ok(())
    }

    pub fn get_clone_password(&self) -> String {
        if self.clone_password.is_empty() {
            return String::new();
        }
        if let Some(plain) = Self::dpapi_unprotect(&self.clone_password) {
            plain
        } else {
            warn!("DPAPI 解密失败 回退字面量");
            // §3.3 三态协议：解密失败返回带 DPAPI: 前缀的磁盘原文字面量（不剥前缀），
            // 内存/磁盘/失败三态 = 明文 / DPAPI:base64 / DPAPI:base64(字面量+warn)
            self.clone_password.clone()
        }
    }

    pub fn set_clone_password(&mut self, plaintext: &str) {
        self.clone_password = if plaintext.is_empty() {
            String::new()
        } else {
            Self::dpapi_protect(plaintext)
        };
    }
}

pub struct SettingsService {
    inner: Arc<RwLock<AppSettings>>,
}

impl SettingsService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppSettings::load_raw())),
        }
    }

    pub fn current(&self) -> AppSettings {
        self.inner.read().unwrap().clone()
    }

    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppSettings),
    {
        let mut guard = self.inner.write().unwrap();
        f(&mut guard);
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let guard = self.inner.read().unwrap();
        guard.save()
    }
}

impl Default for SettingsService {
    fn default() -> Self {
        Self::new()
    }
}