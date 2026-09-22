// AGPL-3.0 许可证

use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU32, Ordering}};
use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM, LRESULT, POINT, HINSTANCE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    RegisterClassExW, ShowWindow, LoadIconW, LoadCursorW, IDC_ARROW, IDI_APPLICATION,
    WM_NCLBUTTONDOWN, WM_CLOSE, WM_DESTROY, WM_QUIT, WM_USER,
    HTCAPTION, WS_POPUP, WS_VISIBLE, WS_SYSMENU, WS_MINIMIZEBOX,
    CW_USEDEFAULT, SW_HIDE, SW_RESTORE,
    WNDCLASSEXW, CS_HREDRAW, CS_VREDRAW,
    GetCursorPos, TrackPopupMenuEx, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TPM_RETURNCMD,
    GetSubMenu, LoadMenuW,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NIM_DELETE,
    NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::core::PCWSTR;
use crate::rdp::host::{RdpHost, RdpEvent};
use crate::settings::{SettingsService, AppSettings};
use crate::session::{is_child_sessions_enabled, logoff_child_session, get_configured_rdp_port, is_rdp_listener_active, is_rdp_wrapper_installed, get_termsrv_version};
use crate::launcher::launch_in_session;
use crate::environment::{run_all_checks, run_all_fixes, CheckResult};
use crossbeam_channel::{bounded, Sender, Receiver};

#[cfg(feature = "gui")]
use tauri::Manager;

const WM_TRAYICON: u32 = WM_USER + 1;
const WM_STATUS_TICK: u32 = WM_USER + 2;
const WM_TOAST_SHOW: u32 = WM_USER + 3;
const WM_TOAST_HIDE: u32 = WM_USER + 4;
const WM_CONNECT: u32 = WM_USER + 5;
const WM_DISCONNECT: u32 = WM_USER + 6;
const WM_TERMINATE: u32 = WM_USER + 7;
const WM_GAME_MOUSE: u32 = WM_USER + 8;
const WM_LAUNCH_PROGRAM: u32 = WM_USER + 9;
const WM_ENV_CHECK: u32 = WM_USER + 10;
const WM_SETTINGS: u32 = WM_USER + 11;
const WM_EXIT: u32 = WM_USER + 12;
const WM_DO_CONNECT: u32 = WM_USER + 13;

static APP_INSTANCE: OnceLock<Arc<App>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Debug, Clone)]
pub struct ConnectionDiagnostics {
    pub server: String,
    pub user: String,
    pub domain: String,
    pub width: String,
    pub height: String,
    pub color: String,
    pub error: String,
}

impl Default for ConnectionDiagnostics {
    fn default() -> Self {
        Self {
            server: String::new(),
            user: String::new(),
            domain: String::new(),
            width: String::new(),
            height: String::new(),
            color: String::new(),
            error: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToastMessage {
    pub title: String,
    pub message: String,
    pub level: ToastLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Warning,
    Error,
}

pub struct RawHwnd(*mut std::ffi::c_void);
unsafe impl Send for RawHwnd {}
unsafe impl Sync for RawHwnd {}
impl Clone for RawHwnd {
    fn clone(&self) -> Self { *self }
}
impl Copy for RawHwnd {}

impl From<HWND> for RawHwnd {
    fn from(h: HWND) -> Self { RawHwnd(h.0) }
}
impl From<RawHwnd> for HWND {
    fn from(r: RawHwnd) -> Self { HWND(r.0) }
}

// 事件桥：Tauri AppHandle 存储 + CPU 差值采样状态，随 gui feature 退化
#[cfg(feature = "gui")]
#[derive(Default)]
struct EventBridge {
    handle: OnceLock<tauri::AppHandle>,
    cpu_prev: Mutex<Option<(u64, u64, u64)>>,
}

#[cfg(not(feature = "gui"))]
#[derive(Default)]
#[allow(dead_code)]
struct EventBridge;

pub struct App {
    hwnd: RawHwnd,
    hinstance: HINSTANCE,
    tray_icon: Mutex<Option<NOTIFYICONDATAW>>,
    rdp_host_ptr: Mutex<Option<*mut RdpHost>>,
    pub settings: Arc<SettingsService>,
    connection_state: Arc<Mutex<ConnectionState>>,
    is_connecting: Arc<AtomicBool>,
    handle_snapshot: Arc<Mutex<Option<RawHwnd>>>,
    event_tx: Sender<AppEvent>,
    event_rx: Mutex<Option<Receiver<AppEvent>>>,
    toast_tx: Sender<ToastMessage>,
    toast_rx: Mutex<Option<Receiver<ToastMessage>>>,
    status_tick_running: Arc<AtomicBool>,
    toast_debounce: Arc<Mutex<HashMap<(String, String), std::time::Instant>>>,
    shutdown_sequence: Arc<AtomicU32>,
    connect_params: Mutex<Option<(String, String, String, String)>>,
    bridge: EventBridge,
}

unsafe impl Send for App {}
unsafe impl Sync for App {}

#[derive(Debug)]
pub enum AppEvent {
    RdpEvent(RdpEvent),
    StatusTick,
    TrayClick(u32),
    TrayDoubleClick,
    Connect,
    Disconnect,
    Terminate,
    GameMouseToggle,
    LaunchProgram,
    EnvCheck,
    Settings,
    Exit,
    ToastShow(ToastMessage),
    ToastHide,
    WindowDrag,
    CloseSequence(u32),
    DoConnect,
}

impl App {
    pub fn instance() -> Arc<Self> {
        APP_INSTANCE.get_or_init(|| Arc::new(Self::new())).clone()
    }

    fn new() -> Self {
        let (event_tx, event_rx) = bounded(128);
        let (toast_tx, toast_rx) = bounded(32);
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None).unwrap().into() };

        let class_name = to_wide("AkiSpaceApp");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
            hIconSm: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        };

        unsafe { RegisterClassExW(&wc) };

        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::from_raw(to_wide("AkiSpace").as_ptr()),
                WS_POPUP | WS_VISIBLE | WS_SYSMENU | WS_MINIMIZEBOX,
                CW_USEDEFAULT, CW_USEDEFAULT, 1024, 768,
                None, None, Some(hinstance.into()), Some(std::ptr::addr_of_mut!(*std::ptr::null_mut())),
            )
        }.expect("主窗口创建失败");

        let tray_icon = Self::create_tray_icon(hwnd);

        let settings = Arc::new(SettingsService::new());

        let app = Self {
            hwnd: hwnd.into(),
            hinstance,
            tray_icon: Mutex::new(Some(tray_icon)),
            rdp_host_ptr: Mutex::new(None),
            settings,
            connection_state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            is_connecting: Arc::new(AtomicBool::new(false)),
            handle_snapshot: Arc::new(Mutex::new(None)),
            event_tx,
            event_rx: Mutex::new(Some(event_rx)),
            toast_tx,
            toast_rx: Mutex::new(Some(toast_rx)),
            status_tick_running: Arc::new(AtomicBool::new(false)),
            toast_debounce: Arc::new(Mutex::new(HashMap::new())),
            shutdown_sequence: Arc::new(AtomicU32::new(0)),
            connect_params: Mutex::new(None),
            bridge: EventBridge::default(),
        };

        app.start_event_loop();
        app.start_status_tick();
        app.start_toast_handler();

        unsafe { ShowWindow(HWND(app.hwnd.0), SW_HIDE) };

        app
    }

    fn create_tray_icon(hwnd: HWND) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
        let tip = to_wide("AkiSpace 远程桌面");
        nid.szTip[..tip.len().min(127)].copy_from_slice(&tip[..tip.len().min(127)]);
        unsafe { Shell_NotifyIconW(NIM_ADD, &mut nid) };
        nid
    }

    fn update_tray_tooltip(&self, text: &str) {
        if let Ok(mut nid) = self.tray_icon.lock() {
            if let Some(ref mut nid) = *nid {
                let tip = to_wide(text);
                nid.szTip[..tip.len().min(127)].copy_from_slice(&tip[..tip.len().min(127)]);
                nid.uFlags = NIF_TIP;
                unsafe { Shell_NotifyIconW(NIM_MODIFY, nid) };
            }
        }
    }

    fn show_tray_menu(&self) {
        let menu_name = to_wide("101");
        let menu = unsafe { LoadMenuW(Some(self.hinstance.into()), PCWSTR::from_raw(menu_name.as_ptr())) };
        let menu = match menu {
            Ok(m) => m,
            Err(_) => return,
        };
        let submenu = unsafe { GetSubMenu(menu, 0) };
        if submenu.0.is_null() {
            return;
        }

        let mut pt = POINT::default();
        unsafe { GetCursorPos(&mut pt) };

        let cmd = unsafe {
            TrackPopupMenuEx(
                submenu,
                TPM_LEFTALIGN.0 | TPM_RIGHTBUTTON.0 | TPM_RETURNCMD.0,
                pt.x, pt.y,
                HWND(self.hwnd.0),
                None,
            )
        };

        if cmd.as_bool() {
            self.handle_tray_command(cmd.0 as u32);
        }
    }

    fn handle_tray_command(&self, cmd: u32) {
        match cmd {
            1001 => self.post_event(AppEvent::Connect),
            1002 => self.post_event(AppEvent::Disconnect),
            1003 => self.post_event(AppEvent::Terminate),
            1004 => self.post_event(AppEvent::GameMouseToggle),
            1005 => self.post_event(AppEvent::LaunchProgram),
            1006 => self.post_event(AppEvent::EnvCheck),
            1007 => self.post_event(AppEvent::Settings),
            1008 => self.post_event(AppEvent::Exit),
            _ => {}
        }
    }

    pub fn post_event(&self, event: AppEvent) {
        let _ = self.event_tx.try_send(event);
    }

    fn start_event_loop(&self) {
        let event_rx = self.event_rx.lock().unwrap().take().unwrap();
        let app = self.clone_inner();

        thread::spawn(move || {
            while let Ok(event) = event_rx.recv() {
                app.handle_event(event);
            }
        });
    }

    fn clone_inner(&self) -> Arc<Self> {
        APP_INSTANCE.get().unwrap().clone()
    }

    fn handle_event(&self, event: AppEvent) {
        match event {
            AppEvent::RdpEvent(rdp_event) => self.handle_rdp_event(rdp_event),
            AppEvent::StatusTick => self.on_status_tick(),
            AppEvent::TrayClick(msg) => self.on_tray_click(msg),
            AppEvent::TrayDoubleClick => self.on_tray_double_click(),
            AppEvent::Connect => self.on_connect(),
            AppEvent::Disconnect => self.on_disconnect(),
            AppEvent::Terminate => self.on_terminate(),
            AppEvent::GameMouseToggle => self.on_game_mouse_toggle(),
            AppEvent::LaunchProgram => self.on_launch_program(),
            AppEvent::EnvCheck => self.on_env_check(),
            AppEvent::Settings => self.on_settings(),
            AppEvent::Exit => self.on_exit(),
            AppEvent::ToastShow(toast) => self.on_toast_show(toast),
            AppEvent::ToastHide => self.on_toast_hide(),
            AppEvent::WindowDrag => self.on_window_drag(),
            AppEvent::CloseSequence(step) => self.on_close_sequence(step),
            AppEvent::DoConnect => self.on_do_connect(),
        }
    }

    fn handle_rdp_event(&self, event: RdpEvent) {
        match event {
            RdpEvent::Connected => {
                *self.connection_state.lock().unwrap() = ConnectionState::Connected;
                self.is_connecting.store(false, Ordering::Relaxed);
                self.update_tray_tooltip("AkiSpace 已连接");
                #[cfg(feature = "gui")]
                self.emit_connection_state();
                self.show_toast("连接成功", "RDP 会话已建立", ToastLevel::Info);
            }
            RdpEvent::Disconnected(reason, diag) => {
                *self.connection_state.lock().unwrap() = ConnectionState::Disconnected;
                self.is_connecting.store(false, Ordering::Relaxed);
                self.update_tray_tooltip("AkiSpace 已断开");
                #[cfg(feature = "gui")]
                self.emit_connection_state();
                self.show_toast("连接断开", &format!("原因: {} 诊断: {}", reason, diag), ToastLevel::Warning);
            }
            RdpEvent::FatalError(code) => {
                *self.connection_state.lock().unwrap() = ConnectionState::Disconnected;
                self.is_connecting.store(false, Ordering::Relaxed);
                #[cfg(feature = "gui")]
                self.emit_connection_state();
                self.show_toast("致命错误", &format!("错误代码: {}", code), ToastLevel::Error);
            }
            RdpEvent::LoginComplete(code) => {
                if code != 0 {
                    self.show_toast("登录失败", &format!("错误代码: {}", code), ToastLevel::Error);
                }
            }
            RdpEvent::Warning(code) => {
                self.show_toast("警告", &format!("警告代码: {}", code), ToastLevel::Warning);
            }
            RdpEvent::AutoReconnecting(attempt, max) => {
                self.show_toast("重连中", &format!("尝试 {}/{}", attempt, max), ToastLevel::Info);
            }
            RdpEvent::AutoReconnected => {
                self.show_toast("重连成功", "RDP 会话已恢复", ToastLevel::Info);
            }
            RdpEvent::AuthenticationFailed => {
                self.show_toast("认证失败", "用户名或密码错误", ToastLevel::Error);
            }
            _ => {}
        }
    }

    fn on_status_tick(&self) {
        let state = *self.connection_state.lock().unwrap();
        let is_connected = matches!(state, ConnectionState::Connected);

        if let Ok(host_guard) = self.rdp_host_ptr.lock() {
            if let Some(ptr) = *host_guard {
                if !ptr.is_null() {
                    let host = unsafe { &*ptr };
                    let hwnd = host.hwnd();
                    if !hwnd.0.is_null() {
                        *self.handle_snapshot.lock().unwrap() = Some(hwnd.into());
                    }
                }
            }
        }

        let event_connected = is_connected;
        let polling_connected = self.check_connection_via_polling();

        let final_connected = event_connected && polling_connected;
        if final_connected != is_connected {
            *self.connection_state.lock().unwrap() = if final_connected {
                ConnectionState::Connected
            } else {
                ConnectionState::Disconnected
            };
            self.update_tray_tooltip(if final_connected { "AkiSpace 已连接" } else { "AkiSpace 已断开" });
            #[cfg(feature = "gui")]
            self.emit_connection_state();
        }

        if self.is_connecting.load(Ordering::Relaxed) {
            let connecting = self.is_connecting.load(Ordering::Relaxed);
            if !connecting {
                *self.connection_state.lock().unwrap() = ConnectionState::Disconnected;
            }
        }

        #[cfg(feature = "gui")]
        self.emit_status_tick();
    }

    fn check_connection_via_polling(&self) -> bool {
        if let Ok(host_guard) = self.rdp_host_ptr.lock() {
            if let Some(ptr) = *host_guard {
                if !ptr.is_null() {
                    let host = unsafe { &*ptr };
                    let hwnd = host.hwnd();
                    return !hwnd.0.is_null();
                }
            }
        }
        false
    }

    fn on_tray_click(&self, msg: u32) {
        if msg == 0x0203 {
            self.post_event(AppEvent::TrayDoubleClick);
        }
    }

    fn on_tray_double_click(&self) {
        unsafe { ShowWindow(HWND(self.hwnd.0), SW_RESTORE) };
        self.set_foreground();
    }

    fn set_foreground(&self) {
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(self.hwnd.0));
        }
    }

    fn on_connect(&self) {
        if self.is_connecting.swap(true, Ordering::Relaxed) {
            return;
        }

        let settings = self.settings.current();
        let server = "127.0.0.1".to_string();
        let user = settings.clone_username.clone();
        let domain = String::new();
        let password = settings.get_clone_password();

        *self.connection_state.lock().unwrap() = ConnectionState::Connecting;
        self.update_tray_tooltip("AkiSpace 连接中...");
        #[cfg(feature = "gui")]
        self.emit_connection_state();

        let host = match RdpHost::new(HWND(self.hwnd.0)) {
            Ok(h) => Box::new(h),
            Err(e) => {
                self.is_connecting.store(false, Ordering::Relaxed);
                *self.connection_state.lock().unwrap() = ConnectionState::Disconnected;
                #[cfg(feature = "gui")]
                self.emit_connection_state();
                self.show_toast("连接失败", &format!("创建 RDP 主机失败: {}", e), ToastLevel::Error);
                return;
            }
        };

        let app = self.clone_inner();
        host.set_event_handler(move |event| {
            let _ = app.event_tx.try_send(AppEvent::RdpEvent(event));
        });

        let ptr = Box::into_raw(host);
        *self.rdp_host_ptr.lock().unwrap() = Some(ptr);
        *self.connect_params.lock().unwrap() = Some((server, user, domain, password));

        self.post_event(AppEvent::DoConnect);
    }

    fn on_do_connect(&self) {
        let params = self.connect_params.lock().unwrap().take();
        let (server, user, domain, password) = match params {
            Some(p) => p,
            None => return,
        };

        let ptr = match self.rdp_host_ptr.lock().unwrap().take() {
            Some(p) => p,
            None => return,
        };

        if ptr.is_null() {
            return;
        }

        let host = unsafe { &*ptr };
        let result = host.connect(&server, &user, &domain, &password);

        *self.rdp_host_ptr.lock().unwrap() = Some(ptr);

        if let Err(e) = result {
            self.is_connecting.store(false, Ordering::Relaxed);
            *self.connection_state.lock().unwrap() = ConnectionState::Disconnected;
            #[cfg(feature = "gui")]
            self.emit_connection_state();
            self.show_toast("连接失败", &format!("{}", e), ToastLevel::Error);
        }
    }

    fn on_disconnect(&self) {
        if let Ok(host_guard) = self.rdp_host_ptr.lock() {
            if let Some(ptr) = *host_guard {
                if !ptr.is_null() {
                    let host = unsafe { &*ptr };
                    let _ = host.disconnect();
                }
            }
        }
        *self.connection_state.lock().unwrap() = ConnectionState::Disconnecting;
        self.update_tray_tooltip("AkiSpace 断开中...");
    }

    fn on_terminate(&self) {
        let _ = logoff_child_session();
        self.show_toast("会话终止", "子会话已注销", ToastLevel::Info);
    }

    fn on_game_mouse_toggle(&self) {
        let enabled = self.settings.current().game_mouse_mode_enabled;
        self.show_toast("游戏鼠标", if enabled { "已启用" } else { "已禁用" }, ToastLevel::Info);
    }

    fn on_launch_program(&self) {
        let settings = self.settings.current();
        if let Some(path) = settings.launch_program_path {
            if let Ok(session_id) = crate::session::get_child_session_id() {
                let _ = launch_in_session(session_id, &path, "");
                self.show_toast("启动程序", &format!("在会话 {} 中启动", session_id), ToastLevel::Info);
            } else {
                self.show_toast("启动失败", "未找到子会话", ToastLevel::Error);
            }
        } else {
            self.show_toast("启动失败", "未配置程序路径", ToastLevel::Warning);
        }
    }

    fn on_env_check(&self) {
        let mut results = Vec::new();

        let child_sessions_result = is_child_sessions_enabled();
        let child_sessions_msg = match child_sessions_result {
            Ok(true) => "子会话: 已启用".to_string(),
            Ok(false) => "子会话: 未启用".to_string(),
            Err(e) => format!("子会话检查失败: {}", e),
        };
        results.push(child_sessions_msg);

        let port = get_configured_rdp_port();
        results.push(format!("RDP 端口: {}", port));

        if is_rdp_listener_active(port) {
            results.push("RDP 监听: 正常".to_string());
        } else {
            results.push("RDP 监听: 异常".to_string());
        }

        if is_rdp_wrapper_installed() {
            results.push("RDP Wrapper: 已安装".to_string());
        } else {
            results.push("RDP Wrapper: 未安装".to_string());
        }

        let termsrv_result = get_termsrv_version();
        let termsrv_msg = match termsrv_result {
            Ok(v) => format!("termsrv.dll: {}", v),
            Err(e) => format!("termsrv.dll 版本获取失败: {}", e),
        };
        results.push(termsrv_msg);

        let msg = results.join("\n");
        self.show_toast("环境检查", &msg, ToastLevel::Info);
    }

    fn on_settings(&self) {
        self.show_toast("设置", "设置面板待实现", ToastLevel::Info);
    }

    fn on_exit(&self) {
        self.shutdown_sequence.store(1, Ordering::Relaxed);
        self.post_event(AppEvent::CloseSequence(1));
    }

    fn on_close_sequence(&self, step: u32) {
        match step {
            1 => {
                if let Ok(host_guard) = self.rdp_host_ptr.lock() {
                    if let Some(ptr) = *host_guard {
                        if !ptr.is_null() {
                            let host = unsafe { &*ptr };
                            let _ = host.disconnect();
                        }
                    }
                }
                self.shutdown_sequence.store(2, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(2));
            }
            2 => {
                if let Ok(mut tray_guard) = self.tray_icon.lock() {
                    if let Some(ref mut nid) = *tray_guard {
                        unsafe { Shell_NotifyIconW(NIM_DELETE, nid) };
                    }
                    *tray_guard = None;
                }
                self.shutdown_sequence.store(3, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(3));
            }
            3 => {
                self.status_tick_running.store(false, Ordering::Relaxed);
                self.shutdown_sequence.store(4, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(4));
            }
            4 => {
                if let Ok(mut rx_guard) = self.event_rx.lock() {
                    *rx_guard = None;
                }
                self.shutdown_sequence.store(5, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(5));
            }
            5 => {
                if let Ok(mut rx_guard) = self.toast_rx.lock() {
                    *rx_guard = None;
                }
                self.shutdown_sequence.store(6, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(6));
            }
            6 => {
                if let Ok(mut host_guard) = self.rdp_host_ptr.lock() {
                    if let Some(ptr) = *host_guard {
                        if !ptr.is_null() {
                            unsafe { drop(Box::from_raw(ptr)) };
                        }
                    }
                    *host_guard = None;
                }
                self.shutdown_sequence.store(7, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(7));
            }
            7 => {
                unsafe { DestroyWindow(HWND(self.hwnd.0)) };
                self.shutdown_sequence.store(8, Ordering::Relaxed);
                self.post_event(AppEvent::CloseSequence(8));
            }
            8 => {
                unsafe { PostMessageW(Some(HWND(self.hwnd.0)), WM_QUIT, WPARAM(0), LPARAM(0)) };
            }
            _ => {}
        }
    }

    fn on_toast_show(&self, toast: ToastMessage) {
        let now = std::time::Instant::now();
        let key = (toast.title.clone(), toast.message.clone());
        let mut debounce = self.toast_debounce.lock().unwrap();
        if let Some(last) = debounce.get(&key) {
            if now.duration_since(*last) < Duration::from_secs(15) {
                return;
            }
        }
        debounce.insert(key, now);
        drop(debounce);

        #[cfg(feature = "gui")]
        self.emit_toast(&toast);

        self.update_tray_tooltip(&format!("{}: {}", toast.title, toast.message));

        let app = self.clone_inner();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(15));
            let _ = app.toast_tx.try_send(ToastMessage {
                title: String::new(),
                message: String::new(),
                level: ToastLevel::Info,
            });
        });
    }

    fn on_toast_hide(&self) {
        self.update_tray_tooltip("AkiSpace");
    }

    fn on_window_drag(&self) {
        unsafe {
            PostMessageW(Some(HWND(self.hwnd.0)), WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
        }
    }

    fn show_toast(&self, title: &str, message: &str, level: ToastLevel) {
        let _ = self.toast_tx.try_send(ToastMessage {
            title: title.into(),
            message: message.into(),
            level,
        });
    }

    fn start_status_tick(&self) {
        self.status_tick_running.store(true, Ordering::Relaxed);
        let app = self.clone_inner();
        thread::spawn(move || {
            while app.status_tick_running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));
                if app.status_tick_running.load(Ordering::Relaxed) {
                    let _ = app.event_tx.try_send(AppEvent::StatusTick);
                }
            }
        });
    }

    fn start_toast_handler(&self) {
        let toast_rx = self.toast_rx.lock().unwrap().take().unwrap();
        let app = self.clone_inner();
        thread::spawn(move || {
            while let Ok(toast) = toast_rx.recv() {
                if toast.title.is_empty() && toast.message.is_empty() {
                    app.on_toast_hide();
                } else {
                    app.on_toast_show(toast);
                }
            }
        });
    }

    pub fn get_handle_snapshot(&self) -> Option<RawHwnd> {
        self.handle_snapshot.lock().unwrap().clone()
    }

    pub fn run_message_loop(&self) {
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).into() {
                DispatchMessageW(&msg);
            }
        }
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_TRAYICON {
            let event = lparam.0 as u32;
            if event == 0x0203 {
                if let Some(app) = APP_INSTANCE.get() {
                    app.post_event(AppEvent::TrayDoubleClick);
                }
            } else if event == 0x0204 || event == 0x0205 {
                if let Some(app) = APP_INSTANCE.get() {
                    app.post_event(AppEvent::TrayClick(event));
                }
            }
            return LRESULT(0);
        }

        if msg == WM_NCLBUTTONDOWN {
            if wparam.0 == HTCAPTION as usize {
                if let Some(app) = APP_INSTANCE.get() {
                    app.post_event(AppEvent::WindowDrag);
                }
                return LRESULT(0);
            }
        }

        if msg == WM_CLOSE {
            if let Some(app) = APP_INSTANCE.get() {
                app.post_event(AppEvent::Exit);
            }
            return LRESULT(0);
        }

        if msg == WM_DESTROY {
            unsafe { PostMessageW(Some(hwnd), WM_QUIT, WPARAM(0), LPARAM(0)) };
            return LRESULT(0);
        }

        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(feature = "gui")]
impl App {
    fn emit(&self, event: &str, payload: impl serde::Serialize + Clone) {
        use tauri::Emitter;
        if let Some(handle) = self.bridge.handle.get() {
            let _ = handle.emit(event, payload);
        }
    }

    fn emit_connection_state(&self) {
        let state = *self.connection_state.lock().unwrap();
        let status = match state {
            ConnectionState::Connected => "connected",
            ConnectionState::Connecting => "connecting",
            // Disconnecting 前端无对应态，并入 disconnected 由后续 RdpEvent::Disconnected 修正
            ConnectionState::Disconnecting | ConnectionState::Disconnected => "disconnected",
        };
        let username = self.settings.current().clone_username;
        let session_id = if matches!(state, ConnectionState::Connected) {
            crate::session::get_child_session_id().ok().map(|id| id.to_string()).unwrap_or_default()
        } else {
            String::new()
        };
        self.emit("connection-state", serde_json::json!({
            "status": status,
            "host": "127.0.0.1",
            "port": get_configured_rdp_port(),
            "username": username,
            "sessionId": if session_id.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(session_id) },
        }));
    }

    fn emit_status_tick(&self) {
        let connected = matches!(*self.connection_state.lock().unwrap(), ConnectionState::Connected);
        self.emit("status-tick", serde_json::json!({
            "cpu": self.sample_cpu_percent(),
            "memory": Self::sample_memory_mb(),
            "network": if connected { "正常" } else { "未连接" },
        }));
    }

    fn emit_toast(&self, toast: &ToastMessage) {
        let level = match toast.level {
            ToastLevel::Info => "info",
            ToastLevel::Warning => "warning",
            ToastLevel::Error => "error",
        };
        let message = if toast.title.is_empty() {
            toast.message.clone()
        } else {
            format!("{}: {}", toast.title, toast.message)
        };
        self.emit("toast", serde_json::json!({ "message": message, "type": level }));
    }

    fn sample_cpu_percent(&self) -> f64 {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetSystemTimes;
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        if unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.is_err() {
            return 0.0;
        }
        let to_u64 = |ft: &FILETIME| ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        let current = (to_u64(&idle), to_u64(&kernel), to_u64(&user));
        let previous = {
            let mut guard = self.bridge.cpu_prev.lock().unwrap();
            std::mem::replace(&mut *guard, Some(current))
        };
        let (pi, pk, pu) = match previous { Some(p) => p, None => return 0.0 };
        // Windows 约定：kernel 计时已含 idle，故 busy = (kernel+user 增量) - idle 增量
        let total = (current.1 - pk) + (current.2 - pu);
        if total == 0 { return 0.0; }
        ((total - (current.0 - pi)) as f64 / total as f64) * 100.0
    }

    fn sample_memory_mb() -> u64 {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX::default();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut status) }.is_err() {
            return 0;
        }
        (status.ullTotalPhys - status.ullAvailPhys) / 1024 / 1024
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn initialize() {
    let app = App::instance();
    app.run_message_loop();
}

pub fn shutdown() {
    if let Some(app) = APP_INSTANCE.get() {
        app.post_event(AppEvent::Exit);
    }
}

#[cfg(feature = "gui")]
pub fn register_app_state(app_handle: &tauri::AppHandle) {
    let app = App::instance();
    let _ = app.bridge.handle.set(app_handle.clone());
    app_handle.manage(app);
}