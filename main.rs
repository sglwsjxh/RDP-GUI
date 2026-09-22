// AGPL-3.0 许可证

use std::env;
use std::process;

use windows::Win32::Foundation::{HANDLE, WIN32_ERROR, GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::PCWSTR;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

#[cfg(feature = "gui")]
use tauri::{Manager, Builder, RunEvent};

#[cfg(feature = "gui")]
use akispace::ipc;

use akispace::{logging, fix_env};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "--fix-env" {
        run_fix_env();
        return;
    }

    // 全局命名互斥锁：Global\ 前缀跨会话可见
    // ponytail: 进程生命周期锁，CloseHandle 泄漏可忽略（进程退出自动回收）
    let mutex_name: Vec<u16> = "Global\\AkiSpace.SingleInstance\0".encode_utf16().collect();
    unsafe {
        let _handle: HANDLE = CreateMutexW(None, true, PCWSTR::from_raw(mutex_name.as_ptr())).expect("CreateMutexW failed");
        if GetLastError() == WIN32_ERROR(ERROR_ALREADY_EXISTS.0) {
            eprintln!("已有实例在运行");
            process::exit(0);
        }
    }

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    logging::init_panic_hook();

    #[cfg(feature = "gui")]
    {
        Builder::default()
            .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }))
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .invoke_handler(ipc::register_commands())
            .setup(|app| {
                logging::init_tracing();
                ipc::setup_app_state(app.handle());
                Ok(())
            })
            .build(tauri::generate_context!())
            .expect("Tauri 构建失败")
            .run(|_app, event| {
                if let RunEvent::ExitRequested { api, .. } = event {
                    api.prevent_exit();
                }
            });
    }

    #[cfg(not(feature = "gui"))]
    {
        eprintln!("GUI 功能未启用 请使用 --features gui 编译");
        std::process::exit(1);
    }
}

fn run_fix_env() {
    fix_env::run_fix_env();
}
