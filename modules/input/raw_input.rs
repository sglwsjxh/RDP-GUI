// AGPL-3.0 许可证

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, HINSTANCE};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RAWINPUTDEVICE_FLAGS, HRAWINPUT, RAW_INPUT_DATA_COMMAND_FLAGS, RIDEV_INPUTSINK, RID_INPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostThreadMessageW, RegisterClassW,
    UnregisterClassW, SetWindowLongPtrW, GetWindowLongPtrW, MSG, WNDCLASSW, WM_INPUT, WM_QUIT, CS_HREDRAW, CS_VREDRAW,
    WINDOW_EX_STYLE, WINDOW_STYLE, GWLP_USERDATA,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::core::PCWSTR;

type Callback = Arc<dyn Fn(RawInputEvent) + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub struct RawInputEvent {
    pub timestamp: u64,
    pub event_type: RawInputType,
    pub data: RawInputData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawInputType {
    Mouse,
    Keyboard,
}

#[derive(Debug, Clone, Copy)]
pub enum RawInputData {
    Mouse { x: i32, y: i32, buttons: u16, wheel_delta: i16 },
    Keyboard { vk: u16, scan_code: u16, flags: u16 },
}

struct Inner {
    callbacks: Vec<Callback>,
    hwnd: isize,
    thread_id: u32,
    running: bool,
}

pub struct RawInputMonitor {
    inner: Arc<Mutex<Inner>>,
}

impl RawInputMonitor {
    pub fn new() -> Result<Self, String> {
        let inner = Arc::new(Mutex::new(Inner {
            callbacks: Vec::new(),
            hwnd: 0,
            thread_id: 0,
            running: false,
        }));

        let monitor = Self { inner: inner.clone() };

        let thread_inner = inner.clone();
        thread::spawn(move || {
            Self::run_message_loop(thread_inner);
        });

        thread::sleep(std::time::Duration::from_millis(100));

        let mut guard = inner.lock().unwrap();
        if guard.hwnd == 0 {
            return Err("窗口创建失败".into());
        }
        guard.running = true;
        Ok(monitor)
    }

    fn run_message_loop(inner: Arc<Mutex<Inner>>) {
        let hinstance = HINSTANCE(unsafe { GetModuleHandleW(None).unwrap().0 });

        let class_name: Vec<u16> = "AkiSpaceRawInput\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(Self::wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        unsafe { RegisterClassW(&wc); }

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(std::ptr::null()),
                WINDOW_STYLE(0),
                0, 0, 0, 0,
                None,
                None,
                Some(hinstance),
                Some(Box::into_raw(Box::new(inner.clone())) as _),
            )
        };

        let hwnd = match hwnd {
            Ok(h) => h,
            Err(_) => return,
        };

        if hwnd.0.is_null() {
            return;
        }

        let thread_id = unsafe { GetCurrentThreadId() };

        let mut guard = inner.lock().unwrap();
        guard.hwnd = hwnd.0 as isize;
        guard.thread_id = thread_id;

        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(inner.clone())) as isize); }

        let devices = [RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,
            dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
            hwndTarget: hwnd,
        }];

        unsafe { RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).ok(); }

        drop(guard);

        let mut msg = MSG::default();
        loop {
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            match ret.0 {
                -1 => break,
                0 => break,
                _ => {
                    unsafe { DispatchMessageW(&msg); }
                }
            }
        }

        let mut guard = inner.lock().unwrap();
        guard.running = false;
        let _hwnd_val = guard.hwnd;
        let class_name = class_name;
        drop(guard);

        unsafe {
            let devices = [RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x02,
                dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
                hwndTarget: HWND(std::ptr::null_mut()),
            }];
            RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32).ok();
            let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance));
        }
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_INPUT {
            let inner_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            if inner_ptr == 0 {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            let inner: &Arc<Mutex<Inner>> = unsafe { &*(inner_ptr as *const Arc<Mutex<Inner>>) };

            let hraw = HRAWINPUT(lparam.0 as _);
            let mut size = 0u32;
            unsafe {
                GetRawInputData(
                    hraw,
                    RAW_INPUT_DATA_COMMAND_FLAGS(RID_INPUT.0),
                    None,
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                );
            }
            if size == 0 {
                return LRESULT(0);
            }
            let mut buffer = vec![0u8; size as usize];
            unsafe {
                GetRawInputData(
                    hraw,
                    RAW_INPUT_DATA_COMMAND_FLAGS(RID_INPUT.0),
                    Some(buffer.as_mut_ptr() as _),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                );
            }
            let raw = unsafe { &*(buffer.as_ptr() as *const RAWINPUT) };

            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;

            let event = match raw.header.dwType {
                0 => { // RIM_TYPEMOUSE
                    let mouse = unsafe { raw.data.mouse };
                    RawInputEvent {
                        timestamp,
                        event_type: RawInputType::Mouse,
                        data: RawInputData::Mouse {
                            x: mouse.lLastX,
                            y: mouse.lLastY,
                            buttons: unsafe { mouse.Anonymous.Anonymous.usButtonFlags },
                            wheel_delta: unsafe { mouse.Anonymous.Anonymous.usButtonData } as i16,
                        },
                    }
                }
                1 => { // RIM_TYPEKEYBOARD
                    let kb = unsafe { raw.data.keyboard };
                    RawInputEvent {
                        timestamp,
                        event_type: RawInputType::Keyboard,
                        data: RawInputData::Keyboard {
                            vk: kb.VKey,
                            scan_code: kb.MakeCode,
                            flags: kb.Flags,
                        },
                    }
                }
                _ => return LRESULT(0),
            };

            let callbacks = {
                let guard = inner.lock().unwrap();
                guard.callbacks.clone()
            };

            for cb in callbacks {
                cb(event);
            }

            return LRESULT(0);
        }

        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(RawInputEvent) + Send + Sync + 'static,
    {
        let mut guard = self.inner.lock().unwrap();
        guard.callbacks.push(Arc::new(callback));
    }

    pub fn stop(&self) {
        let mut guard = self.inner.lock().unwrap();
        if guard.running {
            guard.running = false;
            let thread_id = guard.thread_id;
            drop(guard);
            let _ = unsafe { PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }
}

impl Drop for RawInputMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}