// AGPL-3.0 许可证

use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_MOVE};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use crate::input::raw_input::{RawInputMonitor, RawInputEvent, RawInputType, RawInputData};

const FLUSH_INTERVAL_MS: u64 = 10;
const CLAMP_LIMIT: i32 = 4096;
const POLL_INTERVAL_MS: u64 = 5;

struct BatchState {
    dx: i32,
    dy: i32,
    sequence: u64,
    base_ticks: u64,
    last_flush: Instant,
    last_dx_sign: i32,
    last_dy_sign: i32,
}

impl BatchState {
    fn new() -> Self {
        Self {
            dx: 0,
            dy: 0,
            sequence: 0,
            base_ticks: 0,
            last_flush: Instant::now(),
            last_dx_sign: 0,
            last_dy_sign: 0,
        }
    }

    fn sign(v: i32) -> i32 {
        if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
    }

    fn clamp(v: i32) -> i32 {
        v.max(-CLAMP_LIMIT).min(CLAMP_LIMIT)
    }

    fn should_flush(&self, dx: i32, dy: i32) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_flush).as_millis() as u64;
        if elapsed >= FLUSH_INTERVAL_MS {
            return true;
        }
        let dx_sign = Self::sign(dx);
        let dy_sign = Self::sign(dy);
        if dx_sign != 0 && dx_sign != self.last_dx_sign {
            return true;
        }
        if dy_sign != 0 && dy_sign != self.last_dy_sign {
            return true;
        }
        false
    }

    fn accumulate(&mut self, dx: i32, dy: i32, sequence: u64, base_ticks: u64) {
        self.dx = Self::clamp(self.dx + dx);
        self.dy = Self::clamp(self.dy + dy);
        if self.sequence == 0 {
            self.sequence = sequence;
            self.base_ticks = base_ticks;
        }
        if dx != 0 {
            self.last_dx_sign = Self::sign(dx);
        }
        if dy != 0 {
            self.last_dy_sign = Self::sign(dy);
        }
    }

    fn take_batch(&mut self) -> Option<(i32, i32, u64, u64)> {
        if self.dx == 0 && self.dy == 0 {
            return None;
        }
        let batch = (self.dx, self.dy, self.sequence, self.base_ticks);
        self.dx = 0;
        self.dy = 0;
        self.sequence = 0;
        self.base_ticks = 0;
        self.last_flush = Instant::now();
        self.last_dx_sign = 0;
        self.last_dy_sign = 0;
        Some(batch)
    }
}

struct ForwarderInner {
    monitor: Mutex<Option<RawInputMonitor>>,
    running: AtomicBool,
    sequence: AtomicU64,
    last_sample_ticks: AtomicU64,
    batch: Mutex<BatchState>,
    alt_pressed: AtomicBool,
    focus_lost: AtomicBool,
}

pub struct MouseForwarder {
    inner: Arc<ForwarderInner>,
}

impl MouseForwarder {
    pub fn new() -> Result<Self, String> {
        let inner = Arc::new(ForwarderInner {
            monitor: Mutex::new(None),
            running: AtomicBool::new(false),
            sequence: AtomicU64::new(1),
            last_sample_ticks: AtomicU64::new(0),
            batch: Mutex::new(BatchState::new()),
            alt_pressed: AtomicBool::new(false),
            focus_lost: AtomicBool::new(false),
        });

        let forwarder = Self { inner: inner.clone() };

        let monitor = RawInputMonitor::new()?;
        monitor.subscribe({
            let inner = inner.clone();
            move |event| {
                if let RawInputEvent { event_type: RawInputType::Mouse, data: RawInputData::Mouse { x, y, .. }, timestamp } = event {
                    let mut batch = inner.batch.lock().unwrap();
                    batch.accumulate(x, y, inner.sequence.fetch_add(1, Ordering::Relaxed), timestamp);
                    inner.last_sample_ticks.store(timestamp, Ordering::Relaxed);
                }
            }
        });

        *inner.monitor.lock().unwrap() = Some(monitor);
        inner.running.store(true, Ordering::Relaxed);

        let poll_inner = inner.clone();
        thread::spawn(move || {
            Self::poll_loop(poll_inner);
        });

        let flush_inner = inner.clone();
        thread::spawn(move || {
            Self::flush_loop(flush_inner);
        });

        Ok(forwarder)
    }

    fn poll_loop(inner: Arc<ForwarderInner>) {
        while inner.running.load(Ordering::Relaxed) {
            let alt = unsafe { GetAsyncKeyState(0x12) as u16 } & 0x8000u16 != 0;
            inner.alt_pressed.store(alt, Ordering::Relaxed);

            let fg = unsafe { GetForegroundWindow() };
            let mut fg_pid = 0u32;
            unsafe { GetWindowThreadProcessId(fg, Some(&mut fg_pid)); }
            let cur_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
            inner.focus_lost.store(fg_pid != cur_pid, Ordering::Relaxed);

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    }

    fn flush_loop(inner: Arc<ForwarderInner>) {
        while inner.running.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(FLUSH_INTERVAL_MS));

            let should_flush = {
                let batch = inner.batch.lock().unwrap();
                batch.should_flush(batch.dx, batch.dy)
            };

            if should_flush {
                if let Some((dx, dy, _seq, _base)) = inner.batch.lock().unwrap().take_batch() {
                    if inner.alt_pressed.load(Ordering::Relaxed) || inner.focus_lost.load(Ordering::Relaxed) {
                        continue;
                    }
                    Self::send_mouse_move(dx, dy);
                }
            }
        }
    }

    fn send_mouse_move(dx: i32, dy: i32) {
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: dx as i32,
            dy: dy as i32,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE,
            time: 0,
            dwExtraInfo: 0,
        };
        unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32); }
    }

    pub fn stop(&self) {
        self.inner.running.store(false, Ordering::Relaxed);
        if let Some(monitor) = self.inner.monitor.lock().unwrap().take() {
            monitor.stop();
        }
    }
}

impl Drop for MouseForwarder {
    fn drop(&mut self) {
        self.stop();
    }
}