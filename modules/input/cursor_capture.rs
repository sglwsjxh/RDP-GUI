// AGPL-3.0 许可证

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    ShowCursor, ClipCursor, SetCursorPos, GetClipCursor,
};

const MAX_HIDE_COUNT: i32 = 64;

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

struct CursorCaptureInner {
    capture_rect: RECT,
    old_rect: Option<RECT>,
    hide_count: i32,
    active: bool,
    warn_throttled: WarnThrottled,
}

impl CursorCaptureInner {
    fn new(capture_rect: RECT) -> Self {
        Self {
            capture_rect,
            old_rect: None,
            hide_count: 0,
            active: false,
            warn_throttled: WarnThrottled::new(),
        }
    }

    fn capture(&mut self) -> Result<(), String> {
        if self.active {
            return Ok(());
        }

        let mut current_rect = RECT::default();
        unsafe {
            if GetClipCursor(&mut current_rect).is_ok() {
                if current_rect.left != 0 || current_rect.top != 0 || current_rect.right != 0 || current_rect.bottom != 0 {
                    self.old_rect = Some(current_rect);
                }
            }
        }

        unsafe {
            if ClipCursor(Some(&self.capture_rect)).is_err() {
                self.warn_throttled.warn("ClipCursor 捕获失败");
                return Err("ClipCursor 捕获失败".into());
            }
        }

        for _ in 0..MAX_HIDE_COUNT {
            let count = unsafe { ShowCursor(false) };
            if count < 0 {
                self.hide_count += 1;
            } else {
                break;
            }
        }

        self.active = true;
        self.warn_throttled.reset();
        Ok(())
    }

    fn release_temporarily(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }

        unsafe {
            if ClipCursor(None).is_err() {
                self.warn_throttled.warn("ClipCursor 释放失败");
                return Err("ClipCursor 释放失败".into());
            }
        }

        let center_x = (self.capture_rect.left + self.capture_rect.right) / 2;
        let center_y = (self.capture_rect.top + self.capture_rect.bottom) / 2;
        unsafe {
            if SetCursorPos(center_x, center_y).is_err() {
                self.warn_throttled.warn("SetCursorPos 失败");
            }
        }

        for _ in 0..self.hide_count {
            unsafe { ShowCursor(true); }
        }
        self.hide_count = 0;

        self.active = false;
        self.warn_throttled.reset();
        Ok(())
    }

    fn release(&mut self) -> Result<(), String> {
        if !self.active && self.old_rect.is_none() && self.hide_count == 0 {
            return Ok(());
        }

        if let Some(rect) = self.old_rect.take() {
            unsafe {
                if ClipCursor(Some(&rect)).is_err() {
                    self.warn_throttled.warn("ClipCursor 恢复旧区域失败");
                }
            }
        } else {
            unsafe {
                if ClipCursor(None).is_err() {
                    self.warn_throttled.warn("ClipCursor 清除失败");
                }
            }
        }

        for _ in 0..self.hide_count {
            unsafe { ShowCursor(true); }
        }
        self.hide_count = 0;

        self.active = false;
        self.warn_throttled.reset();
        Ok(())
    }

    fn update_rect(&mut self, rect: RECT) {
        self.capture_rect = rect;
        if self.active {
            unsafe {
                if ClipCursor(Some(&rect)).is_err() {
                    self.warn_throttled.warn("ClipCursor 更新区域失败");
                }
            }
        }
    }
}

pub struct CursorCapture {
    inner: Arc<Mutex<CursorCaptureInner>>,
}

impl CursorCapture {
    pub fn new(capture_rect: RECT) -> Self {
        let inner = Arc::new(Mutex::new(CursorCaptureInner::new(capture_rect)));
        Self { inner }
    }

    pub fn capture(&self) -> Result<(), String> {
        self.inner.lock().unwrap().capture()
    }

    pub fn release_temporarily(&self) -> Result<(), String> {
        self.inner.lock().unwrap().release_temporarily()
    }

    pub fn release(&self) -> Result<(), String> {
        self.inner.lock().unwrap().release()
    }

    pub fn update_rect(&self, rect: RECT) {
        self.inner.lock().unwrap().update_rect(rect);
    }

    pub fn is_active(&self) -> bool {
        self.inner.lock().unwrap().active
    }
}

impl Drop for CursorCapture {
    fn drop(&mut self) {
        let _ = self.release();
    }
}