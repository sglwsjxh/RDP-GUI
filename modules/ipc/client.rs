// AGPL-3.0 许可证
// PipeClient = 子会话 agent 侧 = 握手发起方（§3.9 基线）。
// 握手机器时序（红线 1/2）：
//   读后即删 nonce 文件（拿不到 = 拒连，fail-closed 双端）→ 2s 窗口连管道
//   → 发 Handshake(nonce) → 5s 内读 1 字节 verdict：
//     0x01 = 成功移交；0x00 = 显式拒绝不重试（同 nonce 重试必败）；超时/EOF/IO = 重试（1s）

use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::time::{sleep, timeout, Instant};
use tracing::{debug, error, warn};

use crate::ipc::frame::{
    read_frame_async, write_frame_async, Frame, FrameType, Payload, RelativeMouseBatch,
    RelativeMouseResult, HANDSHAKE_ACCEPTED, NONCE_LEN,
};
use crate::ipc::nonce::{nonce_dir, read_and_delete_nonce};
use crate::ipc::server::PIPE_NAME;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RETRIES: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
enum Ack {
    Accepted,
    Rejected,
    Lost,
}

/// 发起方握手：只发 Handshake、只读 verdict——与 server::verify_handshake 构成单向时序，无互等死锁
pub(crate) async fn perform_handshake<S>(stream: &mut S, nonce: [u8; NONCE_LEN]) -> Ack
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let frame = Frame { frame_type: FrameType::Handshake, payload: Payload::Handshake(nonce) };
    match timeout(CONNECT_TIMEOUT, write_frame_async(stream, &frame)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            warn!("写 Handshake 失败: {e}");
            return Ack::Lost;
        }
        Err(_) => {
            warn!("写 Handshake 超时 ({CONNECT_TIMEOUT:?})");
            return Ack::Lost;
        }
    }
    match timeout(ACK_TIMEOUT, read_frame_async(stream)).await {
        Ok(Ok(Some(f))) => match &f.payload {
            Payload::HandshakeAck(v) if *v == HANDSHAKE_ACCEPTED => Ack::Accepted,
            Payload::HandshakeAck(_) => Ack::Rejected,
            other => {
                warn!("握手期收到意外帧: {other:?}");
                Ack::Lost
            }
        },
        Ok(Ok(None)) => {
            debug!("握手中服务端断开（未发 verdict）");
            Ack::Lost
        }
        Ok(Err(e)) => {
            warn!("握手 verdict 帧非法: {e}");
            Ack::Lost
        }
        Err(_) => {
            warn!("verdict 读取超时 ({ACK_TIMEOUT:?})");
            Ack::Lost
        }
    }
}

pub struct PipeClient {
    nonce_path: PathBuf,
}

impl PipeClient {
    /// nonce_path 可为具体文件或所在目录（目录 = 自动取最新 nonce_<Guid>.bin）
    pub fn new(nonce_path: impl Into<PathBuf>) -> Self {
        Self { nonce_path: nonce_path.into() }
    }

    pub fn with_default_nonce_dir() -> Self {
        Self::new(nonce_dir())
    }

    pub async fn connect(&self) -> Result<NamedPipeConnection> {
        // 双端 fail-closed（红线 2）：无 nonce 硬连只会浪费服务端管道实例并掩盖配置问题
        let nonce = match read_and_delete_nonce(&self.nonce_path) {
            Ok(n) => n,
            Err(e) => {
                error!("无可用 nonce，拒连: {e}");
                return Err(Error::new(e.kind(), format!("无可用 nonce: {e}")));
            }
        };
        let mut last_err = Error::new(ErrorKind::NotConnected, "握手中断");
        for attempt in 0..MAX_RETRIES {
            match Self::open_pipe().await {
                Ok(mut pipe) => match perform_handshake(&mut pipe, nonce).await {
                    Ack::Accepted => return Ok(NamedPipeConnection::new(pipe)),
                    // §3.9：显式拒绝不重试——同 nonce 重试必败
                    Ack::Rejected => {
                        return Err(Error::new(
                            ErrorKind::PermissionDenied,
                            "服务端显式拒绝握手（nonce 不被接受，不重试）",
                        ))
                    }
                    Ack::Lost => {
                        last_err = Error::new(ErrorKind::NotConnected, "握手中断，稍后重试");
                    }
                },
                Err(e) => {
                    debug!("管道不可达: {e}");
                    last_err = e;
                }
            }
            if attempt + 1 < MAX_RETRIES {
                sleep(RETRY_DELAY).await;
            }
        }
        Err(last_err)
    }

    async fn open_pipe() -> Result<NamedPipeClient> {
        let start = Instant::now();
        loop {
            match ClientOptions::new().open(PIPE_NAME) {
                Ok(pipe) => return Ok(pipe),
                Err(e) => {
                    if e.raw_os_error() != Some(ERROR_PIPE_BUSY.0 as i32)
                        && e.kind() != ErrorKind::NotFound
                    {
                        return Err(e);
                    }
                }
            }
            if start.elapsed() >= CONNECT_TIMEOUT {
                return Err(Error::new(ErrorKind::TimedOut, "管道连接超时 ({CONNECT_TIMEOUT:?})"));
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    /// 单次握手（自读 nonce 文件）。true = 接受。完整"连接+重试"请用 connect()。
    pub async fn handshake(&self, conn: &mut NamedPipeConnection) -> Result<bool> {
        let nonce = read_and_delete_nonce(&self.nonce_path)?;
        Ok(perform_handshake(&mut conn.pipe, nonce).await == Ack::Accepted)
    }

    pub async fn send_batch(&self, conn: &mut NamedPipeConnection, batch: RelativeMouseBatch) -> Result<RelativeMouseResult> {
        let frame = Frame {
            frame_type: FrameType::RelativeMouseBatch,
            payload: Payload::RelativeMouseBatch(batch),
        };
        conn.write_frame(&frame).await?;

        let result = timeout(RESPONSE_TIMEOUT, conn.read_result()).await;
        match result {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::new(ErrorKind::TimedOut, "等待结果超时")),
        }
    }

    pub async fn send_json(&self, conn: &mut NamedPipeConnection, json: String) -> Result<String> {
        let frame = Frame {
            frame_type: FrameType::Utf8Json,
            payload: Payload::Utf8Json(json),
        };
        conn.write_frame(&frame).await?;

        let response = timeout(RESPONSE_TIMEOUT, conn.read_json()).await;
        match response {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::new(ErrorKind::TimedOut, "等待响应超时")),
        }
    }
}

pub struct NamedPipeConnection {
    pipe: NamedPipeClient,
}

impl NamedPipeConnection {
    fn new(pipe: NamedPipeClient) -> Self {
        Self { pipe }
    }

    async fn write_frame(&mut self, frame: &Frame) -> Result<()> {
        write_frame_async(&mut self.pipe, frame).await
    }

    async fn read_result(&mut self) -> Result<RelativeMouseResult> {
        loop {
            match read_frame_async(&mut self.pipe).await {
                Ok(Some(frame)) => {
                    if let Payload::RelativeMouseResult(result) = frame.payload {
                        return Ok(result);
                    }
                }
                Ok(None) => return Err(Error::new(ErrorKind::UnexpectedEof, "EOF")),
                Err(e) => return Err(e),
            }
        }
    }

    async fn read_json(&mut self) -> Result<String> {
        loop {
            match read_frame_async(&mut self.pipe).await {
                Ok(Some(frame)) => {
                    if let Payload::Utf8Json(s) = frame.payload {
                        return Ok(s);
                    }
                }
                Ok(None) => return Err(Error::new(ErrorKind::UnexpectedEof, "EOF")),
                Err(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::server::verify_handshake;
    use tokio::io::duplex;

    #[tokio::test]
    async fn handshake_loopback_accept_verdict_one_byte() {
        // 双端真实握手函数同进程对跑：client.perform_handshake ↔ server.verify_handshake
        let nonce = [42u8; NONCE_LEN];
        let (mut s, mut c) = duplex(64 * 1024);
        let (server_ok, ack) = tokio::join!(
            verify_handshake(&mut s, Some(nonce)),
            perform_handshake(&mut c, nonce)
        );
        assert!(server_ok, "正确 nonce：server 必须接受");
        assert_eq!(ack, Ack::Accepted, "正确 nonce：client 必须收到 0x01");
    }

    #[tokio::test]
    async fn handshake_loopback_wrong_nonce_gets_explicit_reject() {
        let (mut s, mut c) = duplex(64 * 1024);
        let (server_ok, ack) = tokio::join!(
            verify_handshake(&mut s, Some([1u8; NONCE_LEN])),
            perform_handshake(&mut c, [2u8; NONCE_LEN])
        );
        assert!(!server_ok);
        assert_eq!(ack, Ack::Rejected, "错误 nonce：必须收到 0x00 显式拒绝而非断线");
    }

    #[tokio::test]
    async fn handshake_lost_peer_is_retryable_not_deadlock() {
        // server 侧无 verdict 回音（EOF）→ Lost（可重试路径），5s 超时行为由 server 侧超时测试证明
        let (s, mut c) = duplex(64 * 1024);
        drop(s);
        let ack = perform_handshake(&mut c, [0u8; NONCE_LEN]).await;
        assert_eq!(ack, Ack::Lost);
    }
}
