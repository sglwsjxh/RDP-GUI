// AGPL-3.0 许可证

use std::io::{Error, ErrorKind, Result};
use std::net::TcpStream;
use std::time::Duration;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::RemoteDesktop::{
    WTSEnableChildSessions, WTSGetChildSessionId, WTSIsChildSessionsEnabled, WTSLogoffSession,
};
use windows::Win32::System::Registry::{
    RegOpenKeyExW, RegQueryValueExW, RegCloseKey, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::core::{PCWSTR, BOOL};

const RDP_REG_PATH: &str = r"SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp";
const TERMSERVICE_REG_PATH: &str = r"SYSTEM\CurrentControlSet\Services\TermService\Parameters";
const PORT_VALUE: &str = "PortNumber";
const SERVICE_DLL_VALUE: &str = "ServiceDll";
const TERMSRV_DLL: &str = r"C:\Windows\System32\termsrv.dll";

pub fn enable_child_sessions() -> Result<()> {
    unsafe {
        let result = WTSEnableChildSessions(true);
        if !result.as_bool() {
            return Err(Error::new(ErrorKind::Other, "启用子会话失败"));
        }
        Ok(())
    }
}

pub fn is_child_sessions_enabled() -> Result<bool> {
    unsafe {
        let mut enabled = BOOL::default();
        let result = WTSIsChildSessionsEnabled(&mut enabled);
        if !result.as_bool() {
            return Err(Error::new(ErrorKind::Other, "查询子会话状态失败"));
        }
        Ok(enabled.as_bool())
    }
}

pub fn get_child_session_id() -> Result<u32> {
    unsafe {
        let mut session_id = 0u32;
        let result = WTSGetChildSessionId(&mut session_id);
        if !result.as_bool() {
            return Err(Error::new(ErrorKind::Other, "获取子会话 ID 失败"));
        }
        Ok(session_id)
    }
}

pub fn logoff_child_session() -> Result<()> {
    unsafe {
        let result = WTSLogoffSession(None, 0, true);
        result.map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
    }
}

fn read_reg_dword(key_path: &str, value_name: &str) -> Result<u32> {
    unsafe {
        let key_path_w: Vec<u16> = key_path.encode_utf16().chain(Some(0)).collect();
        let value_name_w: Vec<u16> = value_name.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        let status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(key_path_w.as_ptr()), Some(0), KEY_READ, &mut key);
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("打开注册表键失败: {:?}", status)));
        }
        let mut data = 0u32;
        let mut data_size = std::mem::size_of::<u32>() as u32;
        let mut value_type = REG_VALUE_TYPE(0);
        let status = RegQueryValueExW(
            key,
            PCWSTR(value_name_w.as_ptr()),
            None,
            Some(&mut value_type),
            Some(&mut data as *mut _ as *mut u8),
            Some(&mut data_size),
        );
        RegCloseKey(key);
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("读取注册表值失败: {:?}", status)));
        }
        if value_type.0 != 4 {
            return Err(Error::new(ErrorKind::InvalidData, "注册表值类型非 DWORD"));
        }
        Ok(data)
    }
}

fn read_reg_string(key_path: &str, value_name: &str) -> Result<String> {
    unsafe {
        let key_path_w: Vec<u16> = key_path.encode_utf16().chain(Some(0)).collect();
        let value_name_w: Vec<u16> = value_name.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        let status = RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(key_path_w.as_ptr()), Some(0), KEY_READ, &mut key);
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("打开注册表键失败: {:?}", status)));
        }
        let mut data_size = 0u32;
        let status = RegQueryValueExW(key, PCWSTR(value_name_w.as_ptr()), None, None, None, Some(&mut data_size));
        if status != WIN32_ERROR(0) {
            RegCloseKey(key);
            return Err(Error::new(ErrorKind::NotFound, format!("查询注册表值大小失败: {:?}", status)));
        }
        let mut buf = vec![0u16; (data_size / 2) as usize];
        let status = RegQueryValueExW(
            key,
            PCWSTR(value_name_w.as_ptr()),
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut data_size),
        );
        RegCloseKey(key);
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("读取注册表字符串失败: {:?}", status)));
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf16_lossy(&buf[..len]))
    }
}

pub fn get_configured_rdp_port() -> u16 {
    match read_reg_dword(RDP_REG_PATH, PORT_VALUE) {
        Ok(port) if port > 0 && port <= 65535 => port as u16,
        Ok(_) => 3389, // 越界值回退
        Err(_) => 3389, // 读取失败回退
    }
}

pub fn is_rdp_wrapper_installed() -> bool {
    read_reg_string(TERMSERVICE_REG_PATH, SERVICE_DLL_VALUE)
        .map(|s| s.to_lowercase().contains("rdpwrap.dll") || s.to_lowercase().contains("termwrap.dll"))
        .unwrap_or(false)
}

pub fn get_termsrv_version() -> Result<String> {
    unsafe {
        let path_w: Vec<u16> = TERMSRV_DLL.encode_utf16().chain(Some(0)).collect();
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), Some(&mut handle));
        if size == 0 {
            return Err(Error::new(ErrorKind::NotFound, "获取版本信息大小失败"));
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path_w.as_ptr()), Some(0), size, buf.as_mut_ptr() as _)
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let mut trans: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut trans_len = 0u32;
        let sub_block_translation = "\\VarFileInfo\\Translation\0";
        let sub_block_translation_w: Vec<u16> = sub_block_translation.encode_utf16().collect();
        VerQueryValueW(
            buf.as_ptr() as _,
            PCWSTR(sub_block_translation_w.as_ptr()),
            &mut trans,
            &mut trans_len,
        );
        if trans.is_null() {
            return Err(Error::new(ErrorKind::Other, "查询翻译信息失败"));
        }
        let trans_ptr = trans as *const u32;
        let lang = *trans_ptr;
        let codepage = *trans_ptr.add(1);
        let sub_block = format!("\\StringFileInfo\\{:04X}{:04X}\\FileVersion\0", lang & 0xFFFF, codepage & 0xFFFF);
        let sub_block_w: Vec<u16> = sub_block.encode_utf16().collect();
        let mut version_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut version_len = 0u32;
        VerQueryValueW(
            buf.as_ptr() as _,
            PCWSTR(sub_block_w.as_ptr()),
            &mut version_ptr,
            &mut version_len,
        );
        if version_ptr.is_null() || version_len == 0 {
            return Err(Error::new(ErrorKind::Other, "查询版本字符串失败"));
        }
        let slice = std::slice::from_raw_parts(version_ptr as *const u16, version_len as usize);
        let version = String::from_utf16_lossy(slice);
        let truncated = version.split('.').take(3).collect::<Vec<_>>().join(".");
        Ok(truncated)
    }
}

pub fn is_rdp_listener_active(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    for _ in 0..3 {
        if TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(1500)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}