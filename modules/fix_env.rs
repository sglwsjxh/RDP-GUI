// AGPL-3.0 许可证

use std::process;
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetConsoleTextAttribute, ReadConsoleInputW, INPUT_RECORD,
    STD_OUTPUT_HANDLE, FOREGROUND_BLUE, FOREGROUND_GREEN, FOREGROUND_RED, FOREGROUND_INTENSITY,
    KEY_EVENT, ATTACH_PARENT_PROCESS, CONSOLE_CHARACTER_ATTRIBUTES,
};
use windows::Win32::Foundation::HANDLE;
use crate::environment::run_all_fixes;

const COLOR_CYAN: u16 = FOREGROUND_BLUE.0 | FOREGROUND_GREEN.0 | FOREGROUND_INTENSITY.0;
const COLOR_YELLOW: u16 = FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_INTENSITY.0;
const COLOR_GREEN: u16 = FOREGROUND_GREEN.0 | FOREGROUND_INTENSITY.0;
const COLOR_RED: u16 = FOREGROUND_RED.0 | FOREGROUND_INTENSITY.0;
const COLOR_WHITE: u16 = FOREGROUND_RED.0 | FOREGROUND_GREEN.0 | FOREGROUND_BLUE.0 | FOREGROUND_INTENSITY.0;

fn set_color(handle: HANDLE, color: u16) {
    unsafe { SetConsoleTextAttribute(handle, CONSOLE_CHARACTER_ATTRIBUTES(color)); }
}

fn print_colored(handle: HANDLE, color: u16, text: &str) {
    set_color(handle, color);
    print!("{}", text);
    set_color(handle, COLOR_WHITE);
}

fn wait_key(handle: HANDLE) {
    unsafe {
        let mut record = INPUT_RECORD::default();
        let mut count = 0u32;
        loop {
            let _ = ReadConsoleInputW(handle, std::slice::from_mut(&mut record), &mut count);
            if count > 0 && record.EventType == KEY_EVENT as u16 {
                let key_event = record.Event.KeyEvent;
                if key_event.bKeyDown.as_bool() {
                    break;
                }
            }
        }
    }
}

pub fn run_fix_env() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS); }

    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE).expect("获取控制台句柄失败") };

    print_colored(handle, COLOR_CYAN, "AkiSpace — 环境修复（管理员）\n\n");

    let results = run_all_fixes();
    let mut fail_count = 0;

    for r in results {
        let step_name = format!("{:?}", r.step);
        if r.success {
            print_colored(handle, COLOR_GREEN, "✓ ");
            print!("{}", step_name);
            if !r.detail.is_empty() {
                print!(" — {}", r.detail);
            }
            println!();
        } else {
            print_colored(handle, COLOR_RED, "✗ ");
            print!("{}", step_name);
            if !r.detail.is_empty() {
                print!(" — {}", r.detail);
            }
            println!();
            fail_count += 1;
        }
    }

    println!();
    if fail_count == 0 {
        print_colored(handle, COLOR_GREEN, "所有修复已成功应用！\n");
    } else {
        print_colored(handle, COLOR_YELLOW, &format!("{} 项修复失败\n", fail_count));
    }

    println!();
    print_colored(handle, COLOR_WHITE, "按任意键退出...");
    wait_key(handle);

    if fail_count > 0 {
        process::exit(1);
    }
}