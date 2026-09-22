// AGPL-3.0 许可证

pub mod raw_input;
pub mod forwarder;
pub mod cursor_capture;
pub mod replay;

pub use raw_input::{RawInputMonitor, RawInputEvent, RawInputType, RawInputData};
pub use forwarder::MouseForwarder;
pub use cursor_capture::CursorCapture;
pub use replay::AgentRunner;

pub fn capture_input() {
    // 输入捕获 待实现
}

pub fn release_input() {
    // 释放输入 待实现
}
