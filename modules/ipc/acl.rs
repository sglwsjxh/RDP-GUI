// AGPL-3.0 许可证

use std::io::{Error, ErrorKind, Result};
use windows::Win32::Foundation::{HANDLE, HLOCAL};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, WinLocalSystemSid, PSID, TOKEN_ACCESS_MASK,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS};

/// PWSTR → String：元素数 = NUL 前 u16 个数（旧写法传 len*2 会越读吞进终止符后的堆垃圾）
unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    let len = (0..).take_while(|&i| *p.0.add(i) != 0).count();
    String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
}

/// 当前用户 token 的 SID 字符串字节（S-1-… 形态）
pub fn current_user_sid() -> Result<Vec<u8>> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ACCESS_MASK(0x0008), &mut token)
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let sid_bytes = sid_of_token(token);
        let _ = windows::Win32::Foundation::CloseHandle(token);
        sid_bytes
    }
}

pub fn system_sid() -> Result<Vec<u8>> {
    unsafe {
        let mut size = 0u32;
        windows::Win32::Security::CreateWellKnownSid(WinLocalSystemSid, None, None, &mut size);
        let mut buf = vec![0u8; size as usize];
        let sid_ptr = PSID(buf.as_mut_ptr() as _);
        windows::Win32::Security::CreateWellKnownSid(WinLocalSystemSid, None, Some(sid_ptr), &mut size)
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        sid_to_string_bytes(sid_ptr)
    }
}

pub fn clone_sid(pid: u32) -> Result<Vec<u8>> {
    unsafe {
        let handle = OpenProcess(PROCESS_ACCESS_RIGHTS(0x0400), false, pid)
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        let result = sid_of_token_via_handle(handle);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        result
    }
}

unsafe fn sid_of_token_via_handle(proc_handle: HANDLE) -> Result<Vec<u8>> {
    let mut token = HANDLE::default();
    OpenProcessToken(proc_handle, TOKEN_ACCESS_MASK(0x0008), &mut token)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let out = sid_of_token(token);
    let _ = windows::Win32::Foundation::CloseHandle(token);
    out
}

unsafe fn sid_of_token(token: HANDLE) -> Result<Vec<u8>> {
    let mut size = 0u32;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
    let mut buf = vec![0u8; size as usize];
    GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as _), size, &mut size)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let token_user = &*(buf.as_ptr() as *const windows::Win32::Security::TOKEN_USER);
    sid_to_string_bytes(token_user.User.Sid)
}

unsafe fn sid_to_string_bytes(sid: PSID) -> Result<Vec<u8>> {
    let mut sid_str = windows::core::PWSTR::null();
    ConvertSidToStringSidW(sid, &mut sid_str)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    let s = pwstr_to_string(sid_str);
    let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(sid_str.0 as _)));
    Ok(s.into_bytes())
}

pub fn allowed_sids(clone_pid: Option<u32>) -> Result<Vec<Vec<u8>>> {
    let mut sids = Vec::new();
    sids.push(current_user_sid()?);
    sids.push(system_sid()?);
    if let Some(pid) = clone_pid {
        sids.push(clone_sid(pid)?);
    }
    Ok(sids)
}

pub fn check_client_sid(client_sid: &[u8], allowed: &[Vec<u8>]) -> bool {
    allowed.iter().any(|a| a == client_sid)
}

/// SDDL 字符串 → 内核可复制的安全描述符 + SECURITY_ATTRIBUTES 包装。
/// SD 指针为系统堆分配（LocalFree 释放）；raw() 返回的 *mut SECURITY_ATTRIBUTES
/// 仅在本 guard 存活且未移动期间有效——建流/建文件调用点即用，不得跨 await 缓存。
pub struct SddlSecurity {
    sd: windows::Win32::Security::PSECURITY_DESCRIPTOR,
    attrs: windows::Win32::Security::SECURITY_ATTRIBUTES,
}

impl SddlSecurity {
    pub fn from_sddl(sddl: &str) -> Result<Self> {
        use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        let mut wide: Vec<u16> = sddl.encode_utf16().collect();
        wide.push(0);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
                windows::core::PCWSTR(wide.as_ptr()),
                windows::Win32::Security::Authorization::SDDL_REVISION_1,
                &mut sd,
                None,
            ).map_err(|e| Error::new(ErrorKind::Other, format!("SDDL 解析失败: {e}")))?;
        }
        Ok(Self {
            sd,
            attrs: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            },
        })
    }

    pub fn raw(&mut self) -> *mut std::ffi::c_void {
        std::ptr::addr_of_mut!(self.attrs).cast()
    }
}

impl Drop for SddlSecurity {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::Foundation::LocalFree(Some(HLOCAL(self.sd.0 as _)));
        }
    }
}

fn valid_sid_string(s: &str) -> bool {
    // SDDL 注入守卫：SID 字符串只允许 S-1-5-… 形态的字符集（十进制子authority）
    !s.is_empty() && s.len() <= 184 && s.starts_with("S-") &&
        s.chars().all(|c| c.is_ascii_digit() || c == '-' || c.eq_ignore_ascii_case(&'s'))
}

/// 白名单 DACL（红线 3 / §3.9）：断继承（D:P 前缀），仅白名单 SID 获得 GENERIC_ALL。
/// 管道建流与 nonce 文件收紧共用。空白名单直接 Err——绝不回退成开放默认 ACL。
pub fn build_pipe_dacl_sd(sids: &[Vec<u8>]) -> Result<SddlSecurity> {
    if sids.is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "ACL 白名单 SID 列表为空"));
    }
    let mut sddl = String::from("D:P");
    for sid in sids {
        let s = String::from_utf8(sid.clone())
            .map_err(|_| Error::new(ErrorKind::InvalidData, "SID 非合法 UTF-8"))?;
        if !valid_sid_string(&s) {
            return Err(Error::new(ErrorKind::InvalidData, format!("SID 字符串形态非法: {s}")));
        }
        sddl.push_str(&format!("(A;;GA;;;{s})"));
    }
    SddlSecurity::from_sddl(&sddl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid_strings_are_clean_well_known() {
        let sys = String::from_utf8(system_sid().unwrap()).unwrap();
        assert_eq!(sys, "S-1-5-18", "system_sid 不得吞进 NUL 后垃圾");
        let me = String::from_utf8(current_user_sid().unwrap()).unwrap();
        assert!(me.starts_with("S-1-") && me.len() >= 9 && valid_sid_string(&me), "{me}");
    }

    #[test]
    fn build_pipe_dacl_sd_smoke_with_system_sid() {
        let sys = system_sid().expect("取 SYSTEM SID 失败");
        let mut sec = build_pipe_dacl_sd(&[sys]).unwrap();
        let raw = sec.raw();
        assert!(!raw.is_null());
        let sa = unsafe { &*(raw as *const windows::Win32::Security::SECURITY_ATTRIBUTES) };
        assert_eq!(sa.nLength as usize, std::mem::size_of::<windows::Win32::Security::SECURITY_ATTRIBUTES>());
        assert!(!sa.lpSecurityDescriptor.is_null());
        assert_eq!(sa.bInheritHandle.0, 0);
    }

    #[test]
    fn build_pipe_dacl_sd_rejects_empty_whitelist() {
        assert!(build_pipe_dacl_sd(&[]).is_err());
    }

    #[test]
    fn build_pipe_dacl_sd_rejects_malformed_sid() {
        // SDDL 注入面：非 S-… 形态的字节串必须拒绝而非拼进 SDDL
        assert!(build_pipe_dacl_sd(&[b"D:P(A;;GA;;;WD)fake".to_vec()]).is_err());
    }
}
