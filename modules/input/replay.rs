// AGPL-3.0 许可证

use std::sync::{Arc, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, INPUT_KEYBOARD, MOUSEINPUT, KEYBDINPUT,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
};
use windows::Win32::System::SystemInformation::GetTickCount64;

const CHANNEL_CAPACITY: usize = 1024;
const GROUP_THRESHOLD_MS: u64 = 8;

struct ReplayEvent {
    sequence: u64,
    timestamp: u64,
    input: INPUT,
}

struct ReplayInner {
    sender: crossbeam_channel::Sender<ReplayEvent>,
    running: AtomicBool,
    sequence: AtomicU64,
    last_flush: Mutex<Instant>,
    group_threshold: Duration,
    warn_throttled: WarnThrottled,
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

pub struct AgentRunner {
    inner: Arc<ReplayInner>,
    worker_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

impl AgentRunner {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(CHANNEL_CAPACITY);

        let inner = Arc::new(ReplayInner {
            sender,
            running: AtomicBool::new(true),
            sequence: AtomicU64::new(1),
            last_flush: Mutex::new(Instant::now()),
            group_threshold: Duration::from_millis(GROUP_THRESHOLD_MS),
            warn_throttled: WarnThrottled::new(),
        });

        let worker_inner = inner.clone();
        let handle = thread::spawn(move || {
            Self::worker_loop(worker_inner, receiver);
        });

        Self {
            inner,
            worker_handle: Mutex::new(Some(handle)),
        }
    }

    fn worker_loop(inner: Arc<ReplayInner>, receiver: crossbeam_channel::Receiver<ReplayEvent>) {
        let mut pending: VecDeque<ReplayEvent> = VecDeque::new();
        let mut current_group: Vec<INPUT> = Vec::new();
        let mut _group_first_seq: u64 = 0;
        let mut _group_base_ticks: u64 = 0;

        while inner.running.load(Ordering::Relaxed) {
            let timeout = Duration::from_millis(1);
            match receiver.recv_timeout(timeout) {
                Ok(event) => {
                    pending.push_back(event);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }

            let should_flush = {
                let last_flush = inner.last_flush.lock().unwrap();
                last_flush.elapsed() >= inner.group_threshold
            };

            if should_flush || pending.len() >= 64 {
                while let Some(event) = pending.pop_front() {
                    if current_group.is_empty() {
                        _group_first_seq = event.sequence;
                        _group_base_ticks = event.timestamp;
                    }
                    current_group.push(event.input);

                    if current_group.len() >= 64 {
                        break;
                    }
                }

                if !current_group.is_empty() {
                    let sent = unsafe {
                        SendInput(
                            &current_group,
                            std::mem::size_of::<INPUT>() as i32,
                        )
                    };

                    if sent as usize != current_group.len() {
                        inner.warn_throttled.warn("SendInput 发送数量不匹配");
                    }

                    current_group.clear();
                    _group_first_seq = 0;
                    _group_base_ticks = 0;
                }

                *inner.last_flush.lock().unwrap() = Instant::now();
            }
        }

        while let Ok(event) = receiver.try_recv() {
            pending.push_back(event);
        }

        while !pending.is_empty() {
            while let Some(event) = pending.pop_front() {
                if current_group.is_empty() {
                    _group_first_seq = event.sequence;
                    _group_base_ticks = event.timestamp;
                }
                current_group.push(event.input);

                if current_group.len() >= 64 {
                    break;
                }
            }

            if !current_group.is_empty() {
                let sent = unsafe {
                    SendInput(
                        &current_group,
                        std::mem::size_of::<INPUT>() as i32,
                    )
                };

                if sent as usize != current_group.len() {
                    inner.warn_throttled.warn("SendInput 发送数量不匹配");
                }

                current_group.clear();
                _group_first_seq = 0;
                _group_base_ticks = 0;
            }
        }
    }

    fn get_tick_count() -> u64 {
        unsafe { GetTickCount64() }
    }

    pub fn send_mouse_move(&self, dx: i32, dy: i32) -> Result<(), String> {
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
        self.enqueue(input)
    }

    pub fn send_mouse_button(&self, down: bool, right: bool) -> Result<(), String> {
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        let flags = if right {
            if down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP }
        } else {
            if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP }
        };
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: 0,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
        self.enqueue(input)
    }

    pub fn send_mouse_wheel(&self, delta: i16) -> Result<(), String> {
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: 0,
            dy: 0,
            mouseData: delta as u32,
            dwFlags: MOUSEEVENTF_WHEEL,
            time: 0,
            dwExtraInfo: 0,
        };
        self.enqueue(input)
    }

    pub fn send_key(&self, vk: u16, scan_code: u16, up: bool) -> Result<(), String> {
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        let mut flags = KEYEVENTF_SCANCODE;
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: scan_code,
            dwFlags: flags,
            time: 0,
            dwExtraInfo: 0,
        };
        self.enqueue(input)
    }

    fn enqueue(&self, input: INPUT) -> Result<(), String> {
        if !self.inner.running.load(Ordering::Relaxed) {
            return Err("Runner 已停止".into());
        }

        let sequence = self.inner.sequence.fetch_add(1, Ordering::Relaxed);
        let timestamp = Self::get_tick_count();

        let event = ReplayEvent {
            sequence,
            timestamp,
            input,
        };

        self.inner.sender.send(event).map_err(|_| "通道已关闭".into())
    }

    pub fn stop(&self) {
        self.inner.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.worker_handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AgentRunner {
    fn drop(&mut self) {
        self.stop();
    }
}