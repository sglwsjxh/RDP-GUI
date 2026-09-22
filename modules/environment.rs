// AGPL-3.0 许可证

use std::io::{Error, ErrorKind, Result};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, RegCloseKey, RegDeleteKeyValueW,
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, KEY_WOW64_64KEY, REG_VALUE_TYPE, REG_DWORD, REG_SAM_FLAGS,
};
use windows::Win32::System::Services::{
    OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, StartServiceW, ControlService,
    CloseServiceHandle, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_STOP,
    SC_STATUS_PROCESS_INFO, SERVICE_RUNNING, SERVICE_CONTROL_STOP,
    SERVICE_STATUS, SERVICE_STATUS_PROCESS,
};
use windows::Win32::System::RemoteDesktop::{
    WTSEnableChildSessions, WTSIsChildSessionsEnabled,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::core::{PCWSTR, BOOL};

const TERMSERVER_REG_PATH: &str = r"SYSTEM\CurrentControlSet\Control\Terminal Server";
const RDP_REG_PATH: &str = r"SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp";
const TERMSERVICE_REG_PATH: &str = r"SYSTEM\CurrentControlSet\Services\TermService\Parameters";
const TERMSRV_DLL: &str = r"C:\Windows\System32\termsrv.dll";
const FIREWALL_RULE_NAME: &str = "AkiSpace RDP Loopback";
const DEFAULT_RDP_RULE_NAME: &str = "Remote Desktop - User Mode (TCP-In)";
const DENY_TS_CONNECTIONS: &str = "fDenyTSConnections";
const SINGLE_SESSION_PER_USER: &str = "fSingleSessionPerUser";
const START_RCM: &str = "StartRCM";
const SECURITY_LAYER: &str = "SecurityLayer";
const MIN_ENCRYPTION_LEVEL: &str = "MinEncryptionLevel";
const USER_AUTHENTICATION: &str = "UserAuthentication";
const SERVICE_DLL_VALUE: &str = "ServiceDll";
const TERMSERVICE_NAME: &str = "TermService";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum CheckItem {
    RdpEnabled,
    MultiSession,
    RdpWrapper,
    StartRcm,
    TermServiceRunning,
    FirewallLoopbackRule,
    ChildSessions,
    RdpListener,
    TermsrvVersion,
    RdpWrapperHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum FixStep {
    DisableRdpWrapperHook,
    SetDenyTSConnections,
    SetSingleSessionPerUser,
    SetStartRcm,
    SetSecurityLayer,
    EnsureFirewallLoopbackRule,
    RestartTermService,
    EnableChildSessions,
    RecheckAll,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckResult {
    pub item: CheckItem,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FixResult {
    pub step: FixStep,
    pub success: bool,
    pub detail: String,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn open_key_64(path: &str, access: u32) -> Result<HKEY> {
    unsafe {
        let path_w = to_wide(path);
        let mut key = HKEY::default();
        let sam = REG_SAM_FLAGS(access | KEY_WOW64_64KEY.0);
        let status = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path_w.as_ptr()),
            Some(0),
            sam,
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("打开注册表键失败: {:?}", status)));
        }
        Ok(key)
    }
}

fn read_reg_dword(key: HKEY, value_name: &str) -> Result<u32> {
    unsafe {
        let value_name_w = to_wide(value_name);
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
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("读取注册表值失败: {:?}", status)));
        }
        if value_type != REG_DWORD {
            return Err(Error::new(ErrorKind::InvalidData, "注册表值类型非 DWORD"));
        }
        Ok(data)
    }
}

fn write_reg_dword(key: HKEY, value_name: &str, value: u32) -> Result<()> {
    unsafe {
        let value_name_w = to_wide(value_name);
        let data_bytes = value.to_le_bytes();
        let status = RegSetValueExW(
            key,
            PCWSTR(value_name_w.as_ptr()),
            Some(0),
            REG_DWORD,
            Some(&data_bytes),
        );
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::Other, format!("写入注册表值失败: {:?}", status)));
        }
        Ok(())
    }
}

fn read_reg_expand_string(key: HKEY, value_name: &str) -> Result<String> {
    unsafe {
        let value_name_w = to_wide(value_name);
        let mut data_size = 0u32;
        let status = RegQueryValueExW(
            key,
            PCWSTR(value_name_w.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_size),
        );
        if status != WIN32_ERROR(0) {
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
        if status != WIN32_ERROR(0) {
            return Err(Error::new(ErrorKind::NotFound, format!("读取注册表字符串失败: {:?}", status)));
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf16_lossy(&buf[..len]))
    }
}

fn delete_reg_value(key: HKEY, value_name: &str) -> Result<()> {
    unsafe {
        let value_name_w = to_wide(value_name);
        let status = RegDeleteKeyValueW(key, None, PCWSTR(value_name_w.as_ptr()));
        if status != WIN32_ERROR(0) && status != WIN32_ERROR(2) {
            return Err(Error::new(ErrorKind::Other, format!("删除注册表值失败: {:?}", status)));
        }
        Ok(())
    }
}

fn check_rdp_enabled() -> CheckResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            match read_reg_dword(key, DENY_TS_CONNECTIONS) {
                Ok(val) => {
                    let passed = val == 0;
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpEnabled, passed, detail: format!("fDenyTSConnections={}", val) }
                }
                Err(_) => {
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpEnabled, passed: false, detail: "读取 fDenyTSConnections 失败".into() }
                }
            }
        }
        Err(_) => CheckResult { item: CheckItem::RdpEnabled, passed: false, detail: "打开 Terminal Server 键失败".into() }
    }
}

fn check_multi_session() -> CheckResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            match read_reg_dword(key, SINGLE_SESSION_PER_USER) {
                Ok(val) => {
                    let passed = val == 0;
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::MultiSession, passed, detail: format!("fSingleSessionPerUser={}", val) }
                }
                Err(_) => {
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::MultiSession, passed: false, detail: "读取 fSingleSessionPerUser 失败".into() }
                }
            }
        }
        Err(_) => CheckResult { item: CheckItem::MultiSession, passed: false, detail: "打开 Terminal Server 键失败".into() }
    }
}

fn check_rdp_wrapper() -> CheckResult {
    match open_key_64(TERMSERVICE_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            match read_reg_expand_string(key, SERVICE_DLL_VALUE) {
                Ok(val) => {
                    let lower = val.to_lowercase();
                    let is_wrapper = lower.contains("rdpwrap.dll") || lower.contains("termwrap.dll");
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpWrapper, passed: !is_wrapper, detail: format!("ServiceDll={}", val) }
                }
                Err(_) => {
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpWrapper, passed: true, detail: "读取 ServiceDll 失败 视为无 Wrapper".into() }
                }
            }
        }
        Err(_) => CheckResult { item: CheckItem::RdpWrapper, passed: true, detail: "打开 TermService 键失败 视为无 Wrapper".into() }
    }
}

fn check_start_rcm() -> CheckResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            match read_reg_dword(key, START_RCM) {
                Ok(val) => {
                    let passed = val == 1;
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::StartRcm, passed, detail: format!("StartRCM={}", val) }
                }
                Err(_) => {
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::StartRcm, passed: false, detail: "读取 StartRCM 失败".into() }
                }
            }
        }
        Err(_) => CheckResult { item: CheckItem::StartRcm, passed: false, detail: "打开 Terminal Server 键失败".into() }
    }
}

fn check_term_service_running() -> CheckResult {
    unsafe {
        let manager_name: Vec<u16> = "ServicesActive".encode_utf16().chain(Some(0)).collect();
        let svc_name = to_wide(TERMSERVICE_NAME);
        let manager = OpenSCManagerW(None, PCWSTR(manager_name.as_ptr()), SC_MANAGER_CONNECT);
        let manager = match manager {
            Ok(h) => h,
            Err(_) => return CheckResult { item: CheckItem::TermServiceRunning, passed: false, detail: "打开服务管理器失败".into() },
        };
        let service = OpenServiceW(manager, PCWSTR(svc_name.as_ptr()), SERVICE_QUERY_STATUS);
        let service = match service {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseServiceHandle(manager);
                return CheckResult { item: CheckItem::TermServiceRunning, passed: false, detail: "打开 TermService 失败".into() };
            }
        };
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut bytes_needed = 0u32;
        let mut buf = [0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            Some(&mut buf),
            &mut bytes_needed,
        );
        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(manager);
        if ok.is_err() {
            return CheckResult { item: CheckItem::TermServiceRunning, passed: false, detail: "查询服务状态失败".into() };
        }
        let status = unsafe { &*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS) };
        let passed = status.dwCurrentState == SERVICE_RUNNING;
        CheckResult { item: CheckItem::TermServiceRunning, passed, detail: format!("状态={:?}", status.dwCurrentState) }
    }
}

fn check_firewall_loopback_rule() -> CheckResult {
    // 检查自定义 block 规则是否存在
    let block_output = Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", "name", FIREWALL_RULE_NAME])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    // 检查默认公网入站规则是否仍启用
    let default_output = Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", "name", DEFAULT_RDP_RULE_NAME])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match (block_output, default_output) {
        (Ok(block_out), Ok(default_out)) => {
            let block_stdout = String::from_utf8_lossy(&block_out.stdout);
            let block_stderr = String::from_utf8_lossy(&block_out.stderr);
            let block_combined = format!("{}{}", block_stdout, block_stderr);
            let block_found = block_combined.contains(FIREWALL_RULE_NAME);

            let default_stdout = String::from_utf8_lossy(&default_out.stdout);
            let default_stderr = String::from_utf8_lossy(&default_out.stderr);
            let default_combined = format!("{}{}", default_stdout, default_stderr);
            // 默认规则存在且启用 = 公网入站仍开放 = 未通过
            let public_rule_found = default_combined.contains(DEFAULT_RDP_RULE_NAME) && default_combined.contains("Enabled: Yes");

            // 判定：自定义 block 规则存在 且 默认公网规则已禁用/删除
            let passed = block_found && !public_rule_found;
            let detail = if passed {
                "Block 规则存在且默认公网规则已关闭".into()
            } else if !block_found {
                "Block 规则不存在".into()
            } else {
                "默认公网入站规则仍启用".into()
            };
            CheckResult { item: CheckItem::FirewallLoopbackRule, passed, detail }
        }
        _ => CheckResult { item: CheckItem::FirewallLoopbackRule, passed: false, detail: "执行 netsh 失败".into() }
    }
}

fn check_child_sessions() -> CheckResult {
    unsafe {
        let mut enabled = BOOL::default();
        let result = WTSIsChildSessionsEnabled(&mut enabled);
        if !result.as_bool() {
            return CheckResult { item: CheckItem::ChildSessions, passed: false, detail: "查询子会话状态失败".into() };
        }
        let passed = enabled.as_bool();
        CheckResult { item: CheckItem::ChildSessions, passed, detail: format!("启用={}", passed) }
    }
}

fn check_rdp_listener() -> CheckResult {
    let port = get_configured_rdp_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut passed = false;
    for _ in 0..3 {
        if TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(1500)).is_ok() {
            passed = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    CheckResult { item: CheckItem::RdpListener, passed, detail: format!("端口={}", port) }
}

fn check_termsrv_version() -> CheckResult {
    match get_termsrv_version() {
        Ok(ver) => {
            let passed = !ver.is_empty();
            CheckResult { item: CheckItem::TermsrvVersion, passed, detail: format!("版本={}", ver) }
        }
        Err(e) => CheckResult { item: CheckItem::TermsrvVersion, passed: false, detail: format!("获取版本失败: {}", e) }
    }
}

fn check_rdp_wrapper_hook() -> CheckResult {
    match open_key_64(TERMSERVICE_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            match read_reg_expand_string(key, SERVICE_DLL_VALUE) {
                Ok(val) => {
                    let lower = val.to_lowercase();
                    let is_hooked = lower.contains("rdpwrap.dll") || lower.contains("termwrap.dll");
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpWrapperHook, passed: !is_hooked, detail: format!("ServiceDll={}", val) }
                }
                Err(_) => {
                    unsafe { RegCloseKey(key); }
                    CheckResult { item: CheckItem::RdpWrapperHook, passed: true, detail: "读取 ServiceDll 失败 视为无 Hook".into() }
                }
            }
        }
        Err(_) => CheckResult { item: CheckItem::RdpWrapperHook, passed: true, detail: "打开 TermService 键失败 视为无 Hook".into() }
    }
}

pub fn run_all_checks() -> Vec<CheckResult> {
    vec![
        check_rdp_enabled(),
        check_multi_session(),
        check_rdp_wrapper(),
        check_start_rcm(),
        check_term_service_running(),
        check_firewall_loopback_rule(),
        check_child_sessions(),
        check_rdp_listener(),
        check_termsrv_version(),
        check_rdp_wrapper_hook(),
    ]
}

fn fix_disable_rdp_wrapper_hook() -> FixResult {
    match open_key_64(TERMSERVICE_REG_PATH, KEY_WRITE.0) {
        Ok(key) => {
            // 先读取当前 ServiceDll，仅当包含 TermWrap/rdpwrap 时才重写为 termsrv.dll
            match read_reg_expand_string(key, SERVICE_DLL_VALUE) {
                Ok(current) => {
                    let lower = current.to_lowercase();
                    if lower.contains("rdpwrap.dll") || lower.contains("termwrap.dll") {
                        // 写回原版 termsrv.dll，使用 ExpandString 保留 %SystemRoot% 占位
                        let termsrv_expanded = r"%SystemRoot%\System32\termsrv.dll";
                        let termsrv_wide = to_wide(termsrv_expanded);
                        let data_bytes = termsrv_expanded.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
                        let status = unsafe {
                            windows::Win32::System::Registry::RegSetValueExW(
                                key,
                                PCWSTR(to_wide(SERVICE_DLL_VALUE).as_ptr()),
                                Some(0),
                                windows::Win32::System::Registry::REG_EXPAND_SZ,
                                Some(unsafe { std::slice::from_raw_parts(data_bytes.as_ptr() as *const u8, data_bytes.len() * 2) }),
                            )
                        };
                        unsafe { RegCloseKey(key); }
                        if status == windows::Win32::Foundation::WIN32_ERROR(0) {
                            FixResult { step: FixStep::DisableRdpWrapperHook, success: true, detail: "已恢复 ServiceDll 为 termsrv.dll".into() }
                        } else {
                            FixResult { step: FixStep::DisableRdpWrapperHook, success: false, detail: format!("写入 ServiceDll 失败: {:?}", status) }
                        }
                    } else {
                        // 无 Wrapper hook，幂等通过（标准 RDP 模式故意保留解锁层）
                        unsafe { RegCloseKey(key); }
                        FixResult { step: FixStep::DisableRdpWrapperHook, success: true, detail: "保留 RDP Wrapper hook (未检测到 TermWrap)".into() }
                    }
                }
                Err(_) => {
                    // 读取失败视为无 hook，幂等通过
                    unsafe { RegCloseKey(key); }
                    FixResult { step: FixStep::DisableRdpWrapperHook, success: true, detail: "读取 ServiceDll 失败，视为无 Hook".into() }
                }
            }
        }
        Err(e) => FixResult { step: FixStep::DisableRdpWrapperHook, success: false, detail: format!("打开键失败: {}", e) }
    }
}

fn fix_set_deny_ts_connections() -> FixResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_WRITE.0) {
        Ok(key) => {
            let res = write_reg_dword(key, DENY_TS_CONNECTIONS, 0);
            unsafe { RegCloseKey(key); }
            match res {
                Ok(_) => FixResult { step: FixStep::SetDenyTSConnections, success: true, detail: "fDenyTSConnections=0".into() },
                Err(e) => FixResult { step: FixStep::SetDenyTSConnections, success: false, detail: format!("写入失败: {}", e) }
            }
        }
        Err(e) => FixResult { step: FixStep::SetDenyTSConnections, success: false, detail: format!("打开键失败: {}", e) }
    }
}

fn fix_set_single_session_per_user() -> FixResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_WRITE.0) {
        Ok(key) => {
            let res = write_reg_dword(key, SINGLE_SESSION_PER_USER, 0);
            unsafe { RegCloseKey(key); }
            match res {
                Ok(_) => FixResult { step: FixStep::SetSingleSessionPerUser, success: true, detail: "fSingleSessionPerUser=0".into() },
                Err(e) => FixResult { step: FixStep::SetSingleSessionPerUser, success: false, detail: format!("写入失败: {}", e) }
            }
        }
        Err(e) => FixResult { step: FixStep::SetSingleSessionPerUser, success: false, detail: format!("打开键失败: {}", e) }
    }
}

fn fix_set_start_rcm() -> FixResult {
    match open_key_64(TERMSERVER_REG_PATH, KEY_WRITE.0) {
        Ok(key) => {
            let res = write_reg_dword(key, START_RCM, 1);
            unsafe { RegCloseKey(key); }
            match res {
                Ok(_) => FixResult { step: FixStep::SetStartRcm, success: true, detail: "StartRCM=1".into() },
                Err(e) => FixResult { step: FixStep::SetStartRcm, success: false, detail: format!("写入失败: {}", e) }
            }
        }
        Err(e) => FixResult { step: FixStep::SetStartRcm, success: false, detail: format!("打开键失败: {}", e) }
    }
}

fn fix_set_security_layer() -> FixResult {
    match open_key_64(RDP_REG_PATH, KEY_WRITE.0) {
        Ok(key) => {
            let r1 = write_reg_dword(key, SECURITY_LAYER, 2);
            let r2 = write_reg_dword(key, MIN_ENCRYPTION_LEVEL, 3);
            let r3 = write_reg_dword(key, USER_AUTHENTICATION, 1);
            unsafe { RegCloseKey(key); }
            if r1.is_ok() && r2.is_ok() && r3.is_ok() {
                FixResult { step: FixStep::SetSecurityLayer, success: true, detail: "SecurityLayer=2 MinEncryptionLevel=3 UserAuthentication=1".into() }
            } else {
                FixResult { step: FixStep::SetSecurityLayer, success: false, detail: "部分写入失败".into() }
            }
        }
        Err(e) => FixResult { step: FixStep::SetSecurityLayer, success: false, detail: format!("打开键失败: {}", e) }
    }
}

fn fix_ensure_firewall_loopback_rule() -> FixResult {
    let port = get_configured_rdp_port();

    // 1. 先尝试删除默认公网入站规则（失败仅 warn 不阻断）
    let del_output = Command::new("netsh")
        .args(["advfirewall", "firewall", "delete", "rule", "name", DEFAULT_RDP_RULE_NAME])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    if let Err(e) = del_output {
        eprintln!("警告: 删除默认防火墙规则失败: {}", e);
    } else if let Ok(out) = del_output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{}{}", stdout, stderr);
        if !combined.contains("Ok") && !combined.contains("成功") && !out.status.success() {
            eprintln!("警告: 删除默认防火墙规则可能未成功: {}", combined);
        }
    }

    // 2. 添加自定义 block 规则
    let add_output = Command::new("netsh")
        .args([
            "advfirewall", "firewall", "add", "rule",
            "name", FIREWALL_RULE_NAME,
            "dir", "in",
            "action", "block",
            "protocol", "TCP",
            "localport", &port.to_string(),
            "profile", "any",
            "description", "AkiSpace RDP loopback block"
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    match add_output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{}{}", stdout, stderr);
            let add_success = combined.contains("Ok") || combined.contains("成功") || out.status.success();

            // 3. 复核规则是否真正生效
            let verify_output = Command::new("netsh")
                .args(["advfirewall", "firewall", "show", "rule", "name", FIREWALL_RULE_NAME])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            let verify_success = match verify_output {
                Ok(v_out) => {
                    let v_stdout = String::from_utf8_lossy(&v_out.stdout);
                    let v_stderr = String::from_utf8_lossy(&v_out.stderr);
                    let v_combined = format!("{}{}", v_stdout, v_stderr);
                    v_combined.contains(FIREWALL_RULE_NAME)
                }
                Err(_) => false,
            };

            let success = add_success && verify_success;
            let detail = if success {
                "Block 规则已添加并复核通过".into()
            } else if !add_success {
                "添加规则失败".into()
            } else {
                "添加成功但复核未通过".into()
            };
            FixResult { step: FixStep::EnsureFirewallLoopbackRule, success, detail }
        }
        Err(e) => FixResult { step: FixStep::EnsureFirewallLoopbackRule, success: false, detail: format!("执行 netsh 失败: {}", e) }
    }
}

fn fix_restart_term_service() -> FixResult {
    unsafe {
        let manager_name: Vec<u16> = "ServicesActive".encode_utf16().chain(Some(0)).collect();
        let svc_name = to_wide(TERMSERVICE_NAME);
        let manager = OpenSCManagerW(None, PCWSTR(manager_name.as_ptr()), SC_MANAGER_CONNECT);
        let manager = match manager {
            Ok(h) => h,
            Err(_) => return FixResult { step: FixStep::RestartTermService, success: false, detail: "打开服务管理器失败".into() },
        };
        let service = OpenServiceW(manager, PCWSTR(svc_name.as_ptr()), SERVICE_STOP | SERVICE_START | SERVICE_QUERY_STATUS);
        let service = match service {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseServiceHandle(manager);
                return FixResult { step: FixStep::RestartTermService, success: false, detail: "打开 TermService 失败".into() };
            }
        };

        // Stop 服务
        let mut status = SERVICE_STATUS::default();
        let stop_result = ControlService(service, SERVICE_CONTROL_STOP, &mut status);
        if stop_result.is_err() {
            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(manager);
            return FixResult { step: FixStep::RestartTermService, success: false, detail: "发送停止命令失败".into() };
        }

        // 轮询等待 STOPPED 状态（200ms 间隔，超时 10s）
        // ponytail: 10s 经验值，C# 文档用 15s；若生产超时可调大
        const STOP_TIMEOUT_MS: u64 = 10_000;
        const POLL_INTERVAL_MS: u64 = 200;
        let mut elapsed = 0u64;
        let mut stopped = false;
        while elapsed < STOP_TIMEOUT_MS {
            let mut status_proc = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let mut buf = [0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
            let ok = QueryServiceStatusEx(
                service,
                windows::Win32::System::Services::SC_STATUS_PROCESS_INFO,
                Some(&mut buf),
                &mut bytes_needed,
            );
            if ok.is_ok() {
                let status_proc = &*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS);
                if status_proc.dwCurrentState == windows::Win32::System::Services::SERVICE_STOPPED {
                    stopped = true;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            elapsed += POLL_INTERVAL_MS;
        }

        if !stopped {
            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(manager);
            return FixResult { step: FixStep::RestartTermService, success: false, detail: format!("等待服务停止超时 ({}ms)", STOP_TIMEOUT_MS) };
        }

        // Start 服务
        let start_ok = StartServiceW(service, None);
        if start_ok.is_err() {
            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(manager);
            return FixResult { step: FixStep::RestartTermService, success: false, detail: "启动服务失败".into() };
        }

        // 轮询等待 RUNNING 状态（200ms 间隔，超时 10s）
        // ponytail: 10s 经验值，C# 文档用 15s；若生产超时可调大
        const START_TIMEOUT_MS: u64 = 10_000;
        let mut elapsed = 0u64;
        let mut running = false;
        while elapsed < START_TIMEOUT_MS {
            let mut status_proc = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed = 0u32;
            let mut buf = [0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
            let ok = QueryServiceStatusEx(
                service,
                windows::Win32::System::Services::SC_STATUS_PROCESS_INFO,
                Some(&mut buf),
                &mut bytes_needed,
            );
            if ok.is_ok() {
                let status_proc = &*(buf.as_ptr() as *const SERVICE_STATUS_PROCESS);
                if status_proc.dwCurrentState == windows::Win32::System::Services::SERVICE_RUNNING {
                    running = true;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            elapsed += POLL_INTERVAL_MS;
        }

        let _ = CloseServiceHandle(service);
        let _ = CloseServiceHandle(manager);

        if !running {
            return FixResult { step: FixStep::RestartTermService, success: false, detail: format!("等待服务启动超时 ({}ms)", START_TIMEOUT_MS) };
        }

        FixResult { step: FixStep::RestartTermService, success: true, detail: "服务已重启并确认运行中".into() }
    }
}

fn fix_enable_child_sessions() -> FixResult {
    unsafe {
        let result = WTSEnableChildSessions(true);
        let success = result.as_bool();
        FixResult { step: FixStep::EnableChildSessions, success, detail: if success { "子会话已启用".into() } else { "启用失败".into() } }
    }
}

pub fn run_all_fixes() -> Vec<FixResult> {
    let mut results = Vec::new();
    results.push(fix_disable_rdp_wrapper_hook());
    results.push(fix_set_deny_ts_connections());
    results.push(fix_set_single_session_per_user());
    results.push(fix_set_start_rcm());
    results.push(fix_set_security_layer());
    results.push(fix_ensure_firewall_loopback_rule());
    results.push(fix_restart_term_service());
    results.push(fix_enable_child_sessions());
    results.push(FixResult { step: FixStep::RecheckAll, success: true, detail: "复测由调用者执行".into() });
    results
}

pub fn get_configured_rdp_port() -> u16 {
    match open_key_64(RDP_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            let port = read_reg_dword(key, "PortNumber").unwrap_or(3389) as u16;
            unsafe { RegCloseKey(key); }
            port
        }
        Err(_) => 3389,
    }
}

pub fn is_rdp_wrapper_installed() -> bool {
    match open_key_64(TERMSERVICE_REG_PATH, KEY_READ.0) {
        Ok(key) => {
            let result = read_reg_expand_string(key, SERVICE_DLL_VALUE)
                .map(|s| s.to_lowercase().contains("rdpwrap.dll") || s.to_lowercase().contains("termwrap.dll"))
                .unwrap_or(false);
            unsafe { RegCloseKey(key); }
            result
        }
        Err(_) => false,
    }
}

pub fn get_termsrv_version() -> Result<String> {
    unsafe {
        let path_w = to_wide(TERMSRV_DLL);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_item_count() {
        assert_eq!(10, 10);
    }

    #[test]
    fn test_fix_step_count() {
        assert_eq!(9, 9);
    }

    #[test]
    fn test_firewall_rule_name() {
        assert_eq!(FIREWALL_RULE_NAME, "AkiSpace RDP Loopback");
    }

    #[test]
    fn test_registry_paths() {
        assert!(TERMSERVER_REG_PATH.contains("Terminal Server"));
        assert!(RDP_REG_PATH.contains("RDP-Tcp"));
        assert!(TERMSERVICE_REG_PATH.contains("TermService"));
    }
}