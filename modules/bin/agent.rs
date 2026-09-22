// AGPL-3.0 许可证

use std::io::{Error, ErrorKind, Result};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::ClientOptions;
use akispace::ipc::frame::{Frame, FrameType, Payload, RelativeMouseBatch, RelativeMouseResult, HANDSHAKE_ACCEPTED, HANDSHAKE_REJECTED, NONCE_LEN, read_frame_async, write_frame_async};
use akispace::ipc::nonce::{nonce_dir, read_and_delete_nonce};
use akispace::input::replay::AgentRunner;
use tokio::time::timeout;

const PIPE_NAME: &str = r"\\.\pipe\akispace_ipc";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const VERDICT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

async fn perform_handshake<S>(stream: &mut S, nonce: [u8; NONCE_LEN]) -> Result<u8>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use akispace::ipc::frame::{write_frame_async, read_frame_async, Frame, FrameType, Payload, HANDSHAKE_ACCEPTED, HANDSHAKE_REJECTED, NONCE_LEN};
    use tokio::time::timeout;

    // 发送 Handshake(nonce)
    let handshake_frame = Frame {
        frame_type: FrameType::Handshake,
        payload: Payload::Handshake(nonce),
    };
    write_frame_async(stream, &handshake_frame).await?;

    // 读取 verdict (1 字节)
    let verdict_frame = timeout(VERDICT_TIMEOUT, read_frame_async(stream)).await??;
    if let Some(frame) = verdict_frame {
        if frame.frame_type == FrameType::HandshakeAck {
            if let Payload::HandshakeAck(verdict) = frame.payload {
                return Ok(verdict);
            }
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "握手裁决无效"))
}

async fn handle_connection(mut stream: impl AsyncRead + AsyncWrite + Unpin) -> Result<()> {
    use akispace::input::replay::AgentRunner;
    use akispace::ipc::frame::{read_frame_async, write_frame_async, Frame, FrameType, Payload, RelativeMouseBatch, RelativeMouseResult};
    use tokio::time::timeout;

    let runner = AgentRunner::new();

    loop {
        match timeout(std::time::Duration::from_millis(100), read_frame_async(&mut stream)).await {
            Ok(Ok(Some(frame))) => {
                match frame.frame_type {
                    FrameType::RelativeMouseBatch => {
                        if let Payload::RelativeMouseBatch(batch) = frame.payload {
                            // 将批次转发给 AgentRunner
                            for event in batch.events {
                                let dx = event.dx;
                                let dy = event.dy;
                                let _ = runner.send_mouse_move(dx, dy);
                            }
                            // 发送结果
                            let result_frame = Frame {
                                frame_type: FrameType::RelativeMouseResult,
                                payload: Payload::RelativeMouseResult(RelativeMouseResult {
                                    last_seq: batch.first_seq + batch.count as u64 - 1,
                                    handled: true,
                                }),
                            };
                            write_frame_async(&mut stream, &Frame {
                                frame_type: FrameType::RelativeMouseResult,
                                payload: Payload::RelativeMouseResult(RelativeMouseResult {
                                    last_seq: batch.first_seq + batch.count as u64 - 1,
                                    handled: true,
                                }),
                            }).await?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Ok(None)) => break, // EOF
            Ok(Err(e)) => {
                eprintln!("读取帧错误: {}", e);
                break;
            }
            Err(_) => {
                // timeout, continue
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // 读取 nonce 文件
    let nonce_path = akispace::ipc::nonce::nonce_dir().join("nonce.bin");
    let nonce = akispace::ipc::nonce::read_and_delete_nonce(&nonce_path)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "nonce 文件不存在"))?;

    // 连接到管道（作为客户端）
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, async {
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new()
                .open(PIPE_NAME)
            {
                Ok(stream) => break Ok::<_, Error>(stream),
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }).await??;

    // 执行握手
    let mut nonce_bytes = [0u8; 32];
    nonce_bytes.copy_from_slice(&nonce);

    let verdict = perform_handshake(&mut stream, nonce_bytes).await?;
    if verdict != akispace::ipc::frame::HANDSHAKE_ACCEPTED {
        return Err(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "握手被拒绝").into());
    }

    // 连接成功，开始处理连接
    handle_connection(stream).await?;

    Ok(())
}