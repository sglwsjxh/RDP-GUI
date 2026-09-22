// AGPL-3.0 许可证

pub mod frame;
pub mod acl;
pub mod nonce;
pub mod server;
pub mod client;

#[cfg(feature = "gui")]
use std::sync::Arc;
#[cfg(feature = "gui")]
use tauri::{State, AppHandle, Manager, ipc::InvokeHandler};
#[cfg(feature = "gui")]
use crate::app::App;
#[cfg(feature = "gui")]
use crate::settings::AppSettings;
#[cfg(feature = "gui")]
use crate::environment::{run_all_checks, run_all_fixes, CheckResult};

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_get_settings(app: State<'_, Arc<App>>) -> Result<AppSettings, String> {
    Ok(app.settings.current())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_set_settings(app: State<'_, Arc<App>>, settings: AppSettings) -> Result<(), String> {
    app.settings.update(|s| *s = settings);
    app.settings.save().map_err(|e| e.to_string())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_connect(app: State<'_, Arc<App>>) -> Result<(), String> {
    app.post_event(crate::app::AppEvent::Connect);
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_disconnect(app: State<'_, Arc<App>>) -> Result<(), String> {
    app.post_event(crate::app::AppEvent::Disconnect);
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_terminate(app: State<'_, Arc<App>>) -> Result<(), String> {
    app.post_event(crate::app::AppEvent::Terminate);
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_game_mouse_toggle(app: State<'_, Arc<App>>) -> Result<bool, String> {
    let mut new_state = false;
    app.settings.update(|s| {
        s.game_mouse_mode_enabled = !s.game_mouse_mode_enabled;
        new_state = s.game_mouse_mode_enabled;
    });
    app.settings.save().map_err(|e| e.to_string())?;
    app.post_event(crate::app::AppEvent::GameMouseToggle);
    Ok(new_state)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_agent_start(_app: State<'_, Arc<App>>) -> Result<(), String> {
    let _ = std::process::Command::new("akispace-agent")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_launch_program(app: State<'_, Arc<App>>) -> Result<(), String> {
    app.post_event(crate::app::AppEvent::LaunchProgram);
    Ok(())
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_env_check(_app: State<'_, Arc<App>>) -> Result<Vec<CheckResult>, String> {
    let results = run_all_checks();
    Ok(results)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_apply_fixes(_app: State<'_, Arc<App>>) -> Result<Vec<crate::environment::FixResult>, String> {
    let results = run_all_fixes();
    Ok(results)
}

#[cfg(feature = "gui")]
#[tauri::command]
pub async fn cmd_report_viewer_rect(
    app: State<'_, Arc<App>>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), String> {
    if let Some(hwnd) = app.get_handle_snapshot() {
        let hwnd: windows::Win32::Foundation::HWND = hwnd.into();
        if !hwnd.0.is_null() {
            use windows::Win32::UI::WindowsAndMessaging::MoveWindow;
            unsafe {
                let _ = MoveWindow(hwnd, x, y, width, height, true);
            }
        }
    }
    Ok(())
}

#[cfg(feature = "gui")]
pub fn register_commands() -> Box<InvokeHandler<tauri::Wry>> {
    Box::new(tauri::generate_handler![
        cmd_get_settings,
        cmd_set_settings,
        cmd_connect,
        cmd_disconnect,
        cmd_terminate,
        cmd_game_mouse_toggle,
        cmd_agent_start,
        cmd_launch_program,
        cmd_env_check,
        cmd_apply_fixes,
        cmd_report_viewer_rect,
    ])
}

#[cfg(feature = "gui")]
pub fn setup_app_state(app_handle: &AppHandle) {
    crate::app::register_app_state(app_handle);
}

#[cfg(not(feature = "gui"))]
pub fn register_commands() {
}

#[cfg(not(feature = "gui"))]
pub fn setup_app_state(_app_handle: &()) {
}