// AGPL-3.0 许可证

use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicI32, Ordering}};
use std::thread;
use std::time::Duration;
use windows::core::{GUID, IUnknown, Interface, Result, HRESULT, BSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, RECT, HINSTANCE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED, CLSCTX_INPROC_SERVER,
    IConnectionPointContainer, IDispatch,
    DISPPARAMS, DISPATCH_PROPERTYPUT, DISPATCH_METHOD,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetClientRect, MoveWindow, ShowWindow, SW_SHOW,
    WS_CHILD, WS_VISIBLE, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WINDOW_EX_STYLE, WINDOW_STYLE, SHOW_WINDOW_CMD,
};

const CLSID_MS_RDP_CLIENT: GUID = GUID::from_u128(0xA0C63C30_F08D_4AB4_907C_34905D770C7D);
const IID_IMS_TSC_AX_EVENTS: GUID = GUID::from_u128(0x4A5C8F70_8B8B_4A8B_9C8D_5E6F7A8B9C0D);

#[repr(C)]
#[derive(Clone, Copy)]
struct IMsTscAxEventsVtbl {
    on_connected: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_disconnected: unsafe extern "system" fn(this: *mut c_void, reason: i32) -> HRESULT,
    on_login_complete: unsafe extern "system" fn(this: *mut c_void, error_code: i32) -> HRESULT,
    on_fatal_error: unsafe extern "system" fn(this: *mut c_void, error_code: i32) -> HRESULT,
    on_warning: unsafe extern "system" fn(this: *mut c_void, warning_code: i32) -> HRESULT,
    on_remote_desktop_size_change: unsafe extern "system" fn(this: *mut c_void, width: i32, height: i32) -> HRESULT,
    on_idle_timeout_notification: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_request_container_minimize: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_confirm_close: unsafe extern "system" fn(this: *mut c_void, pf_allow_close: *mut i32) -> HRESULT,
    on_received_capabilities: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_connection_bar_pull_down: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_network_bandwidth_changed: unsafe extern "system" fn(this: *mut c_void, quality_level: i32) -> HRESULT,
    on_auto_reconnecting: unsafe extern "system" fn(this: *mut c_void, attempt: i32, max_attempts: i32) -> HRESULT,
    on_auto_reconnected: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_authentication_failed: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_service_message_received: unsafe extern "system" fn(this: *mut c_void, _data: *mut c_void) -> HRESULT,
    on_tunnel_created: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
    on_tunnel_destroyed: unsafe extern "system" fn(this: *mut c_void) -> HRESULT,
}

#[derive(Clone)]
struct EventSink {
    vtbl: &'static IMsTscAxEventsVtbl,
    ref_count: Arc<AtomicI32>,
    handler: Arc<Mutex<Option<Box<dyn Fn(RdpEvent) + Send + Sync>>>>,
    diag_server: Arc<Mutex<String>>,
    diag_user: Arc<Mutex<String>>,
    diag_domain: Arc<Mutex<String>>,
    diag_width: Arc<Mutex<String>>,
    diag_height: Arc<Mutex<String>>,
    diag_color: Arc<Mutex<String>>,
    diag_error: Arc<Mutex<String>>,
}

#[derive(Clone, Debug)]
pub enum RdpEvent {
    Connected,
    Disconnected(i32, String),
    LoginComplete(i32),
    FatalError(i32),
    Warning(i32),
    RemoteDesktopSizeChange(i32, i32),
    IdleTimeoutNotification,
    RequestContainerMinimize,
    ConfirmClose,
    ReceivedCapabilities,
    ConnectionBarPullDown,
    NetworkBandwidthChanged(i32),
    AutoReconnecting(i32, i32),
    AutoReconnected,
    AuthenticationFailed,
    ServiceMessageReceived,
    TunnelCreated,
    TunnelDestroyed,
}

impl EventSink {
    fn new(
        handler: Arc<Mutex<Option<Box<dyn Fn(RdpEvent) + Send + Sync>>>>,
        diag_server: Arc<Mutex<String>>,
        diag_user: Arc<Mutex<String>>,
        diag_domain: Arc<Mutex<String>>,
        diag_width: Arc<Mutex<String>>,
        diag_height: Arc<Mutex<String>>,
        diag_color: Arc<Mutex<String>>,
        diag_error: Arc<Mutex<String>>,
    ) -> Self {
        static VTBL: IMsTscAxEventsVtbl = IMsTscAxEventsVtbl {
            on_connected: EventSink::on_connected,
            on_disconnected: EventSink::on_disconnected,
            on_login_complete: EventSink::on_login_complete,
            on_fatal_error: EventSink::on_fatal_error,
            on_warning: EventSink::on_warning,
            on_remote_desktop_size_change: EventSink::on_remote_desktop_size_change,
            on_idle_timeout_notification: EventSink::on_idle_timeout_notification,
            on_request_container_minimize: EventSink::on_request_container_minimize,
            on_confirm_close: EventSink::on_confirm_close,
            on_received_capabilities: EventSink::on_received_capabilities,
            on_connection_bar_pull_down: EventSink::on_connection_bar_pull_down,
            on_network_bandwidth_changed: EventSink::on_network_bandwidth_changed,
            on_auto_reconnecting: EventSink::on_auto_reconnecting,
            on_auto_reconnected: EventSink::on_auto_reconnected,
            on_authentication_failed: EventSink::on_authentication_failed,
            on_service_message_received: EventSink::on_service_message_received,
            on_tunnel_created: EventSink::on_tunnel_created,
            on_tunnel_destroyed: EventSink::on_tunnel_destroyed,
        };
        Self {
            vtbl: &VTBL,
            ref_count: Arc::new(AtomicI32::new(1)),
            handler,
            diag_server,
            diag_user,
            diag_domain,
            diag_width,
            diag_height,
            diag_color,
            diag_error,
        }
    }

    unsafe extern "system" fn on_connected(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::Connected);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_disconnected(this: *mut c_void, reason: i32) -> HRESULT {
        let sink = this as *mut Self;
        let diag = {
            let server = (*sink).diag_server.lock().unwrap().clone();
            let user = (*sink).diag_user.lock().unwrap().clone();
            let domain = (*sink).diag_domain.lock().unwrap().clone();
            let width = (*sink).diag_width.lock().unwrap().clone();
            let height = (*sink).diag_height.lock().unwrap().clone();
            let color = (*sink).diag_color.lock().unwrap().clone();
            let error = (*sink).diag_error.lock().unwrap().clone();
            format!("server={} user={} domain={} width={} height={} color={} error={}", server, user, domain, width, height, color, error)
        };
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::Disconnected(reason, diag));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_login_complete(this: *mut c_void, error_code: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::LoginComplete(error_code));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_fatal_error(this: *mut c_void, error_code: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::FatalError(error_code));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_warning(this: *mut c_void, warning_code: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::Warning(warning_code));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_remote_desktop_size_change(this: *mut c_void, width: i32, height: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::RemoteDesktopSizeChange(width, height));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_idle_timeout_notification(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::IdleTimeoutNotification);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_request_container_minimize(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::RequestContainerMinimize);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_confirm_close(this: *mut c_void, pf_allow_close: *mut i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::ConfirmClose);
        }
        if !pf_allow_close.is_null() {
            *pf_allow_close = 1;
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_received_capabilities(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::ReceivedCapabilities);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_connection_bar_pull_down(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::ConnectionBarPullDown);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_network_bandwidth_changed(this: *mut c_void, quality_level: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::NetworkBandwidthChanged(quality_level));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_auto_reconnecting(this: *mut c_void, attempt: i32, max_attempts: i32) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::AutoReconnecting(attempt, max_attempts));
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_auto_reconnected(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::AutoReconnected);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_authentication_failed(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::AuthenticationFailed);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_service_message_received(this: *mut c_void, _data: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::ServiceMessageReceived);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_tunnel_created(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::TunnelCreated);
        }
        HRESULT(0)
    }

    unsafe extern "system" fn on_tunnel_destroyed(this: *mut c_void) -> HRESULT {
        let sink = this as *mut Self;
        if let Some(h) = &*(*sink).handler.lock().unwrap() {
            h(RdpEvent::TunnelDestroyed);
        }
        HRESULT(0)
    }
}

unsafe impl Interface for EventSink {
    type Vtable = IMsTscAxEventsVtbl;
    const IID: GUID = IID_IMS_TSC_AX_EVENTS;

    fn as_raw(&self) -> *mut c_void {
        self as *const _ as *mut c_void
    }

    fn into_raw(self) -> *mut c_void {
        let boxed = Box::new(self);
        Box::into_raw(boxed) as *mut c_void
    }

    unsafe fn from_raw(raw: *mut c_void) -> Self {
        let boxed = Box::from_raw(raw as *mut Self);
        *boxed
    }
}

struct WarnThrottled {
    warned: AtomicBool,
}

impl WarnThrottled {
    fn new() -> Self {
        Self { warned: AtomicBool::new(false) }
    }

    fn warn(&self, msg: &str) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            eprintln!("{}", msg);
        }
    }

    fn reset(&self) {
        self.warned.store(false, Ordering::Relaxed);
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub struct RdpHost {
    hwnd: HWND,
    control: Option<IUnknown>,
    cookie: u32,
    event_handler: Arc<Mutex<Option<Box<dyn Fn(RdpEvent) + Send + Sync>>>>,
    warn_throttled: WarnThrottled,
    use_fallback: bool,
    fallback_hwnd: HWND,
    diag_server: Arc<Mutex<String>>,
    diag_user: Arc<Mutex<String>>,
    diag_domain: Arc<Mutex<String>>,
    diag_width: Arc<Mutex<String>>,
    diag_height: Arc<Mutex<String>>,
    diag_color: Arc<Mutex<String>>,
    diag_error: Arc<Mutex<String>>,
}

impl RdpHost {
    pub fn new(parent_hwnd: HWND) -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
            OleInitialize(None).ok();
        }

        let event_handler = Arc::new(Mutex::new(None));
        let warn_throttled = WarnThrottled::new();

        let class_name = to_wide("STATIC");
        let hinstance = unsafe { GetModuleHandleW(None).unwrap() };
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0),
                0, 0, 0, 0,
                Some(parent_hwnd),
                None,
                Some(HINSTANCE(hinstance.0)),
                Some(ptr::null()),
            )
        };

        let hwnd = match hwnd {
            Ok(h) => h,
            Err(_) => {
                warn_throttled.warn("宿主窗口创建失败 使用原生子窗口兜底");
                return Self::create_fallback(parent_hwnd, event_handler, warn_throttled);
            }
        };

        if hwnd.0.is_null() {
            warn_throttled.warn("宿主窗口创建失败 使用原生子窗口兜底");
            return Self::create_fallback(parent_hwnd, event_handler, warn_throttled);
        }

        let control: Result<IUnknown> = unsafe {
            CoCreateInstance(&CLSID_MS_RDP_CLIENT, None, CLSCTX_INPROC_SERVER)
        };

        let control = match control {
            Ok(c) => c,
            Err(_) => {
                warn_throttled.warn("MsRdpClient11 创建失败 使用原生子窗口兜底");
                return Self::create_fallback(parent_hwnd, event_handler, warn_throttled);
            }
        };

        let diag_server = Arc::new(Mutex::new(String::new()));
        let diag_user = Arc::new(Mutex::new(String::new()));
        let diag_domain = Arc::new(Mutex::new(String::new()));
        let diag_width = Arc::new(Mutex::new(String::new()));
        let diag_height = Arc::new(Mutex::new(String::new()));
        let diag_color = Arc::new(Mutex::new(String::new()));
        let diag_error = Arc::new(Mutex::new(String::new()));

        let mut cookie = 0;
        let mut use_fallback = false;

        let cpc: Result<IConnectionPointContainer> = control.cast();
        if let Ok(cpc) = cpc {
            let sink = EventSink::new(
                event_handler.clone(),
                diag_server.clone(),
                diag_user.clone(),
                diag_domain.clone(),
                diag_width.clone(),
                diag_height.clone(),
                diag_color.clone(),
                diag_error.clone(),
            );
            let sink_unknown = unsafe { IUnknown::from_raw(sink.into_raw()) };
            let cp_result = unsafe {
                cpc.FindConnectionPoint(&IID_IMS_TSC_AX_EVENTS)
            };
            if let Ok(cp) = cp_result {
                let advise_result = unsafe {
                    cp.Advise(&sink_unknown)
                };
                match advise_result {
                    Ok(c) => cookie = c,
                    Err(_) => {
                        warn_throttled.warn("ConnectionPoint Advise 失败");
                        use_fallback = true;
                    }
                }
            } else {
                warn_throttled.warn("FindConnectionPoint 失败");
                use_fallback = true;
            }
        } else {
            warn_throttled.warn("IConnectionPointContainer 转换失败");
            use_fallback = true;
        }

        if use_fallback {
            return Self::create_fallback(parent_hwnd, event_handler, warn_throttled);
        }

        let mut rect = RECT::default();
        unsafe { GetClientRect(parent_hwnd, &mut rect).ok(); }
        unsafe {
            MoveWindow(hwnd, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, true).ok();
            ShowWindow(hwnd, SHOW_WINDOW_CMD(SW_SHOW.0)).ok();
        }

        Ok(Self {
            hwnd,
            control: Some(control),
            cookie,
            event_handler,
            warn_throttled,
            use_fallback: false,
            fallback_hwnd: HWND(ptr::null_mut()),
            diag_server,
            diag_user,
            diag_domain,
            diag_width,
            diag_height,
            diag_color,
            diag_error,
        })
    }

    fn create_fallback(parent_hwnd: HWND, event_handler: Arc<Mutex<Option<Box<dyn Fn(RdpEvent) + Send + Sync>>>>, warn_throttled: WarnThrottled) -> Result<Self> {
        let class_name = to_wide("STATIC");
        let hinstance = unsafe { GetModuleHandleW(None).unwrap() };
        let fallback_hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0),
                0, 0, 0, 0,
                Some(parent_hwnd),
                None,
                Some(HINSTANCE(hinstance.0)),
                Some(ptr::null()),
            )
        };

        let fallback_hwnd = match fallback_hwnd {
            Ok(h) => h,
            Err(_) => {
                warn_throttled.warn("原生子窗口创建失败");
                HWND(ptr::null_mut())
            }
        };

        if fallback_hwnd.0.is_null() {
            warn_throttled.warn("原生子窗口创建失败");
        }

        let mut rect = RECT::default();
        unsafe { GetClientRect(parent_hwnd, &mut rect).ok(); }
        unsafe {
            MoveWindow(fallback_hwnd, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, true).ok();
            ShowWindow(fallback_hwnd, SHOW_WINDOW_CMD(SW_SHOW.0)).ok();
        }

        Ok(Self {
            hwnd: HWND(ptr::null_mut()),
            control: None,
            cookie: 0,
            event_handler,
            warn_throttled,
            use_fallback: true,
            fallback_hwnd,
            diag_server: Arc::new(Mutex::new(String::new())),
            diag_user: Arc::new(Mutex::new(String::new())),
            diag_domain: Arc::new(Mutex::new(String::new())),
            diag_width: Arc::new(Mutex::new(String::new())),
            diag_height: Arc::new(Mutex::new(String::new())),
            diag_color: Arc::new(Mutex::new(String::new())),
            diag_error: Arc::new(Mutex::new(String::new())),
        })
    }

    pub fn set_event_handler<F>(&self, handler: F)
    where
        F: Fn(RdpEvent) + Send + Sync + 'static,
    {
        *self.event_handler.lock().unwrap() = Some(Box::new(handler));
    }

    pub fn resize(&self, width: i32, height: i32) {
        let target_hwnd = if self.use_fallback { self.fallback_hwnd } else { self.hwnd };
        if !target_hwnd.0.is_null() {
            unsafe {
                MoveWindow(target_hwnd, 0, 0, width, height, true).ok();
            }
        }
    }

    pub fn connect(&self, server: &str, user: &str, domain: &str, password: &str) -> Result<()> {
        if self.use_fallback || self.control.is_none() {
            return Err(windows::core::Error::new(HRESULT(-2147467259), "未初始化 RDP 控件"));
        }

        let ctrl = self.control.as_ref().unwrap();
        let server_w = to_wide(server);
        let user_w = to_wide(user);
        let domain_w = to_wide(domain);
        let password_w = to_wide(password);

        unsafe {
            let server_bstr = BSTR::from_wide(&server_w);
            let user_bstr = BSTR::from_wide(&user_w);
            let domain_bstr = BSTR::from_wide(&domain_w);
            let password_bstr = BSTR::from_wide(&password_w);

            let server_prop = to_wide("Server");
            let user_prop = to_wide("UserName");
            let domain_prop = to_wide("Domain");
            let connect_method = to_wide("Connect");

            let disp_id_server = Self::get_disp_id(ctrl, &server_prop)?;
            let disp_id_user = Self::get_disp_id(ctrl, &user_prop)?;
            let disp_id_domain = Self::get_disp_id(ctrl, &domain_prop)?;
            let disp_id_connect = Self::get_disp_id(ctrl, &connect_method)?;

            Self::put_property(ctrl, disp_id_server, &server_bstr)?;
            if !user.is_empty() {
                Self::put_property(ctrl, disp_id_user, &user_bstr)?;
            }
            if !domain.is_empty() {
                Self::put_property(ctrl, disp_id_domain, &domain_bstr)?;
            }

            let desktop_width_prop = to_wide("DesktopWidth");
            let desktop_height_prop = to_wide("DesktopHeight");
            let color_depth_prop = to_wide("ColorDepth");
            let connecting_text_prop = to_wide("ConnectingText");
            let disconnected_text_prop = to_wide("DisconnectedText");

            let disp_id_desktop_width = Self::get_disp_id(ctrl, &desktop_width_prop)?;
            let disp_id_desktop_height = Self::get_disp_id(ctrl, &desktop_height_prop)?;
            let disp_id_color_depth = Self::get_disp_id(ctrl, &color_depth_prop)?;
            let disp_id_connecting_text = Self::get_disp_id(ctrl, &connecting_text_prop)?;
            let disp_id_disconnected_text = Self::get_disp_id(ctrl, &disconnected_text_prop)?;

            let width_bstr = BSTR::from_wide(&to_wide("1920"));
            let height_bstr = BSTR::from_wide(&to_wide("1080"));
            let color_bstr = BSTR::from_wide(&to_wide("32"));
            let connecting_bstr = BSTR::from_wide(&to_wide("正在连接..."));
            let disconnected_bstr = BSTR::from_wide(&to_wide("已断开连接"));

            Self::put_property(ctrl, disp_id_desktop_width, &width_bstr)?;
            Self::put_property(ctrl, disp_id_desktop_height, &height_bstr)?;
            Self::put_property(ctrl, disp_id_color_depth, &color_bstr)?;
            Self::put_property(ctrl, disp_id_connecting_text, &connecting_bstr)?;
            Self::put_property(ctrl, disp_id_disconnected_text, &disconnected_bstr)?;

            *self.diag_server.lock().unwrap() = server.to_string();
            *self.diag_user.lock().unwrap() = user.to_string();
            *self.diag_domain.lock().unwrap() = domain.to_string();
            *self.diag_width.lock().unwrap() = "1920".to_string();
            *self.diag_height.lock().unwrap() = "1080".to_string();
            *self.diag_color.lock().unwrap() = "32".to_string();

            let secured_settings2_prop = to_wide("SecuredSettings2");
            let disp_id_secured_settings2 = Self::get_disp_id(ctrl, &secured_settings2_prop)?;
            let secured_settings2: IUnknown = Self::get_property_object(ctrl, disp_id_secured_settings2)?;

            let keyboard_hook_prop = to_wide("KeyboardHookMode");
            let audio_redir_prop = to_wide("AudioRedirectionMode");
            let disp_id_keyboard_hook = Self::get_disp_id(&secured_settings2, &keyboard_hook_prop)?;
            let disp_id_audio_redir = Self::get_disp_id(&secured_settings2, &audio_redir_prop)?;

            let keyboard_bstr = BSTR::from_wide(&to_wide("1"));
            let audio_bstr = BSTR::from_wide(&to_wide("1"));
            Self::put_property(&secured_settings2, disp_id_keyboard_hook, &keyboard_bstr)?;
            Self::put_property(&secured_settings2, disp_id_audio_redir, &audio_bstr)?;

            let adv7_prop = to_wide("AdvancedSettings7");
            let disp_id_adv7 = Self::get_disp_id(ctrl, &adv7_prop)?;
            let adv7: IUnknown = Self::get_property_object(ctrl, disp_id_adv7)?;

            let rdp_port_prop = to_wide("RDPPort");
            let credssp_prop = to_wide("EnableCredSspSupport");
            let win_key_prop = to_wide("EnableWindowsKey");
            let smart_sizing_prop = to_wide("SmartSizing");
            let disp_id_rdp_port = Self::get_disp_id(&adv7, &rdp_port_prop)?;
            let disp_id_credssp = Self::get_disp_id(&adv7, &credssp_prop)?;
            let disp_id_win_key = Self::get_disp_id(&adv7, &win_key_prop)?;
            let disp_id_smart_sizing = Self::get_disp_id(&adv7, &smart_sizing_prop)?;

            let port_bstr = BSTR::from_wide(&to_wide("3389"));
            let credssp_bstr = BSTR::from_wide(&to_wide("1"));
            let win_key_bstr = BSTR::from_wide(&to_wide("1"));
            let smart_sizing_bstr = BSTR::from_wide(&to_wide("1"));
            Self::put_property(&adv7, disp_id_rdp_port, &port_bstr)?;
            Self::put_property(&adv7, disp_id_credssp, &credssp_bstr)?;
            Self::put_property(&adv7, disp_id_win_key, &win_key_bstr)?;
            Self::put_property(&adv7, disp_id_smart_sizing, &smart_sizing_bstr)?;

            let adv9_prop = to_wide("AdvancedSettings9");
            let disp_id_adv9 = Self::get_disp_id(ctrl, &adv9_prop)?;
            let adv9: IUnknown = Self::get_property_object(ctrl, disp_id_adv9)?;

            let auth_level_prop = to_wide("AuthenticationLevel");
            let redirect_clip_prop = to_wide("RedirectClipboard");
            let redirect_drives_prop = to_wide("RedirectDrives");
            let redirect_devices_prop = to_wide("RedirectDevices");
            let redirect_printers_prop = to_wide("RedirectPrinters");
            let redirect_smartcards_prop = to_wide("RedirectSmartCards");
            let perf_flags_prop = to_wide("PerformanceFlags");
            let disp_id_auth_level = Self::get_disp_id(&adv9, &auth_level_prop)?;
            let disp_id_redirect_clip = Self::get_disp_id(&adv9, &redirect_clip_prop)?;
            let disp_id_redirect_drives = Self::get_disp_id(&adv9, &redirect_drives_prop)?;
            let disp_id_redirect_devices = Self::get_disp_id(&adv9, &redirect_devices_prop)?;
            let disp_id_redirect_printers = Self::get_disp_id(&adv9, &redirect_printers_prop)?;
            let disp_id_redirect_smartcards = Self::get_disp_id(&adv9, &redirect_smartcards_prop)?;
            let disp_id_perf_flags = Self::get_disp_id(&adv9, &perf_flags_prop)?;

            let auth_bstr = BSTR::from_wide(&to_wide("2"));
            let false_bstr = BSTR::from_wide(&to_wide("0"));
            let perf_bstr = BSTR::from_wide(&to_wide("143"));
            Self::put_property(&adv9, disp_id_auth_level, &auth_bstr)?;
            Self::put_property(&adv9, disp_id_redirect_clip, &false_bstr)?;
            Self::put_property(&adv9, disp_id_redirect_drives, &false_bstr)?;
            Self::put_property(&adv9, disp_id_redirect_devices, &false_bstr)?;
            Self::put_property(&adv9, disp_id_redirect_printers, &false_bstr)?;
            Self::put_property(&adv9, disp_id_redirect_smartcards, &false_bstr)?;
            Self::put_property(&adv9, disp_id_perf_flags, &perf_bstr)?;

            let adv8_prop = to_wide("AdvancedSettings8");
            let disp_id_adv8 = Self::get_disp_id(ctrl, &adv8_prop)?;
            let adv8: IUnknown = Self::get_property_object(ctrl, disp_id_adv8)?;

            let net_type_prop = to_wide("NetworkConnectionType");
            let disp_id_net_type = Self::get_disp_id(&adv8, &net_type_prop)?;
            let net_type_bstr = BSTR::from_wide(&to_wide("6"));
            Self::put_property(&adv8, disp_id_net_type, &net_type_bstr)?;

            let non_scriptable_prop = to_wide("MsRdpClientNonScriptable");
            let disp_id_non_scriptable = Self::get_disp_id(ctrl, &non_scriptable_prop)?;
            let non_scriptable: IUnknown = Self::get_property_object(ctrl, disp_id_non_scriptable)?;

            let clear_pwd_prop = to_wide("ClearTextPassword");
            let disp_id_clear_pwd = Self::get_disp_id(&non_scriptable, &clear_pwd_prop)?;
            Self::put_property(&non_scriptable, disp_id_clear_pwd, &password_bstr)?;

            let wts_prop = to_wide("WTSEnableChildSessions");
            let disp_id_wts = Self::get_disp_id(ctrl, &wts_prop)?;
            let wts_bstr = BSTR::from_wide(&to_wide("1"));
            Self::put_property(ctrl, disp_id_wts, &wts_bstr)?;

            let ext_prop = to_wide("MsRdpExtendedSettings");
            let disp_id_ext = Self::get_disp_id(ctrl, &ext_prop)?;
            let ext: IUnknown = Self::get_property_object(ctrl, disp_id_ext)?;

            let child_prop = to_wide("ConnectToChildSession");
            let disp_id_child = Self::get_disp_id(&ext, &child_prop)?;
            let child_bstr = BSTR::from_wide(&to_wide("1"));
            Self::put_property(&ext, disp_id_child, &child_bstr)?;

            let child_read = Self::get_property_variant(&ext, disp_id_child)?;
            let child_diag = Self::variant_to_bool_str(&child_read);
            *self.diag_error.lock().unwrap() = format!("child_session={}", child_diag);

            let mut last_err = None;
            let delays = [1, 2, 4];
            for (attempt, &delay) in delays.iter().enumerate() {
                match Self::invoke_method(ctrl, disp_id_connect, &[]) {
                    Ok(_) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        if attempt < delays.len() - 1 {
                            thread::sleep(Duration::from_secs(delay));
                        }
                    }
                }
            }

            if let Some(e) = last_err {
                return Err(e);
            }
        }

        Ok(())
    }

    fn get_property_object(ctrl: &IUnknown, disp_id: i32) -> Result<IUnknown> {
        let dispatch: IDispatch = ctrl.cast()?;
        let mut variant = VARIANT::default();
        let disp_params = DISPPARAMS {
            rgvarg: ptr::null_mut(),
            rgdispidNamedArgs: ptr::null_mut(),
            cArgs: 0,
            cNamedArgs: 0,
        };
        unsafe {
            dispatch.Invoke(disp_id, &GUID::zeroed(), 0, DISPATCH_METHOD, &disp_params, Some(&mut variant), None, None)?;
        }
        let obj: IUnknown = unsafe {
            let punk = &variant.Anonymous.Anonymous.Anonymous.punkVal;
            let punk_ptr: *mut c_void = std::mem::transmute_copy(punk);
            IUnknown::from_raw(punk_ptr)
        };
        Ok(obj)
    }

    fn get_disp_id(ctrl: &IUnknown, name: &[u16]) -> Result<i32> {
        let dispatch: IDispatch = ctrl.cast()?;
        let mut disp_id = 0;
        unsafe {
            dispatch.GetIDsOfNames(&GUID::zeroed(), name.as_ptr() as *const PCWSTR, 1, 0, &mut disp_id)?;
        }
        Ok(disp_id)
    }

    fn put_property(ctrl: &IUnknown, disp_id: i32, value: &BSTR) -> Result<()> {
        let dispatch: IDispatch = ctrl.cast()?;
        let mut variant = VARIANT::default();
        unsafe {
            let variant_ptr = &mut variant as *mut VARIANT;
            let vt_ptr = variant_ptr as *mut u16;
            ptr::write(vt_ptr, windows::Win32::System::Variant::VT_BSTR.0 as u16);
            let bstr_ptr = variant_ptr.add(1) as *mut BSTR;
            ptr::write(bstr_ptr, value.clone());
        }
        let disp_params = DISPPARAMS {
            rgvarg: &mut variant,
            rgdispidNamedArgs: &mut (disp_id as i32),
            cArgs: 1,
            cNamedArgs: 1,
        };
        unsafe {
            dispatch.Invoke(disp_id, &GUID::zeroed(), 0, DISPATCH_PROPERTYPUT, &disp_params, None, None, None)?;
        }
        Ok(())
    }

    fn invoke_method(ctrl: &IUnknown, disp_id: i32, args: &[VARIANT]) -> Result<()> {
        let dispatch: IDispatch = ctrl.cast()?;
        let mut disp_params = DISPPARAMS {
            rgvarg: args.as_ptr() as *mut VARIANT,
            rgdispidNamedArgs: ptr::null_mut(),
            cArgs: args.len() as u32,
            cNamedArgs: 0,
        };
        unsafe {
            dispatch.Invoke(disp_id, &GUID::zeroed(), 0, DISPATCH_METHOD, &disp_params, None, None, None)?;
        }
        Ok(())
    }

    fn get_property_variant(ctrl: &IUnknown, disp_id: i32) -> Result<VARIANT> {
        let dispatch: IDispatch = ctrl.cast()?;
        let mut variant = VARIANT::default();
        let disp_params = DISPPARAMS {
            rgvarg: ptr::null_mut(),
            rgdispidNamedArgs: ptr::null_mut(),
            cArgs: 0,
            cNamedArgs: 0,
        };
        unsafe {
            dispatch.Invoke(disp_id, &GUID::zeroed(), 0, DISPATCH_METHOD, &disp_params, Some(&mut variant), None, None)?;
        }
        Ok(variant)
    }

    fn variant_to_bool_str(variant: &VARIANT) -> String {
        unsafe {
            let vt = variant.Anonymous.Anonymous.vt.0;
            if vt == windows::Win32::System::Variant::VT_BOOL.0 {
                let val = variant.Anonymous.Anonymous.Anonymous.boolVal;
                return if val.0 != 0 { "true".into() } else { "false".into() };
            }
            if vt == windows::Win32::System::Variant::VT_I2.0 {
                let val = variant.Anonymous.Anonymous.Anonymous.iVal;
                return if val != 0 { "true".into() } else { "false".into() };
            }
            format!("vt={} unknown", vt)
        }
    }

    pub fn disconnect(&self) -> Result<()> {
        if self.use_fallback || self.control.is_none() {
            return Ok(());
        }

        let ctrl = self.control.as_ref().unwrap();
        let disconnect_method = to_wide("Disconnect");
        let disp_id = Self::get_disp_id(ctrl, &disconnect_method)?;
        Self::invoke_method(ctrl, disp_id, &[])?;
        Ok(())
    }

    pub fn hwnd(&self) -> HWND {
        if self.use_fallback {
            self.fallback_hwnd
        } else {
            self.hwnd
        }
    }

    pub fn is_fallback(&self) -> bool {
        self.use_fallback
    }
}

impl Drop for RdpHost {
    fn drop(&mut self) {
        if self.cookie != 0 && !self.use_fallback {
            if let Some(ref ctrl) = self.control {
                let cpc: Result<IConnectionPointContainer> = ctrl.cast();
                if let Ok(cpc) = cpc {
                    if let Ok(cp) = unsafe { cpc.FindConnectionPoint(&IID_IMS_TSC_AX_EVENTS) } {
                        let _ = unsafe { cp.Unadvise(self.cookie) };
                    }
                }
            }
        }
    }
}