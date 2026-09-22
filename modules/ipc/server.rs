// AGPL-3.0 许可证
// PipeServer = 主会话侧 = 握手接收方/校验方（§3.9 基线）。
// 握手机器时序（红线 2）：
//   accept（建流即烧 DACL，maxInstances=1）→ 2s 接入窗 → 客户端 SID 纵深校验
//   → 5s 内读 client 的 Handshake(nonce) → 恒定时间比对期望 nonce（未注入则 fail-closed）
//   → 无论成败必发 1 字节 verdict（0x01/0x00，1s 尽力写，失败静默）
//   → 拒绝则 dispose+continue；监听循环任何错误 warn+continue，绝不退出进程。

use std::io::{ErrorKind, Result};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::time::{sleep, timeout};
use tracing::{debug, error, warn};

use crate::ipc::acl::{allowed_sids, build_pipe_dacl_sd, check_client_sid};
use crate::ipc::frame::{
    read_frame_async, write_frame_async, Frame, FrameType, Payload, RelativeMouseResult,
    const_eq, HANDSHAKE_ACCEPTED, HANDSHAKE_REJECTED, NONCE_LEN,
};

/// §3.9 管道名：裸名不带 \\.\pipe\ 前缀（API 自动补；两端一致，禁单侧改）
pub const PIPE_NAME: &str = "AkiSpace_MouseForward";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const VERDICT_TIMEOUT: Duration = Duration::from_secs(1);
const ACCEPT_WINDOW: Duration = Duration::from_secs(2);

pub struct PipeServer {
    allowed_sids: Arc<Vec<Vec<u8>>>,
    expected_nonce: Mutex<Option<[u8; NONCE_LEN]>>,
}

/// §3.9 VerifyHandshakeAsync 等价实现。泛型化以便 duplex 双端回环单测。
/// 无论校验成败都会尽力发出 verdict 帧（让客户端区分"显式拒绝"与"网络断开"）。
pub(crate) async fn verify_handshake<S>(stream: &mut S, expected: Option<[u8; NONCE_LEN]>) -> bool
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut verdict = HANDSHAKE_REJECTED;
    match expected {
        None => warn!("未配置期望 nonce，拒绝握手（fail-closed）"),
        Some(exp) => match timeout(HANDSHAKE_TIMEOUT, read_frame_async(stream)).await {
            Ok(Ok(Some(frame))) => match &frame.payload {
                Payload::Handshake(n) => {
                    if const_eq(n, &exp) {
                        verdict = HANDSHAKE_ACCEPTED;
                    } else {
                        warn!("nonce 比对失败，拒绝握手");
                    }
                }
                other => warn!("握手阶段收到非 Handshake 帧: {:?}", other),
            },
            Ok(Ok(None)) => debug!("握手完成前客户端已断开"),
            Ok(Err(e)) => warn!("握手帧读取错误: {e}"),
            Err(_) => warn!("握手读取超时 ({HANDSHAKE_TIMEOUT:?})"),
        },
    }
    send_verdict(stream, verdict).await;
    verdict == HANDSHAKE_ACCEPTED
}

/// §3.9 SendHandshakeAckAsync：1s 尽力写，任何失败静默——拒绝路径绝不抛回监听循环
async fn send_verdict<S: AsyncWrite + Unpin>(stream: &mut S, verdict: u8) {
    let frame = Frame { frame_type: FrameType::HandshakeAck, payload: Payload::HandshakeAck(verdict) };
    match timeout(VERDICT_TIMEOUT, write_frame_async(stream, &frame)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => debug!("verdict 发送失败（客户端可能已消失，断开本身即 verdict）: {e}"),
        Err(_) => debug!("verdict 发送超时 ({VERDICT_TIMEOUT:?})"),
    }
}

impl PipeServer {
    /// 不变量（红线 3）：SID 白名单在构造时算定，先于任何 accept/Start——DACL 建流即烧死
    pub fn new(clone_pid: Option<u32>) -> Result<Self> {
        Ok(Self {
            allowed_sids: Arc::new(allowed_sids(clone_pid)?),
            expected_nonce: Mutex::new(None),
        })
    }

    /// C# SetNonce 语义：未注入 = fail-closed 拒绝一切握手
    pub fn set_nonce(&self, nonce: [u8; NONCE_LEN]) {
        *self.expected_nonce.lock().unwrap() = Some(nonce);
    }

    pub async fn run(&self) {
        loop {
            match self.accept_connected().await {
                Ok(mut pipe) => {
                    if !Self::client_sid_authorized(&pipe, &self.allowed_sids) {
                        warn!("客户端 SID 不在白名单，丢弃该管道实例");
                        continue;
                    }
                    let expected = *self.expected_nonce.lock().unwrap();
                    if verify_handshake(&mut pipe, expected).await {
                        Self::handle_connection(pipe).await;
                    }
                    // 失败：verdict 已尽力发出；drop 即 dispose，continue 重新监听
                }
                Err(e) => {
                    // §3.9 异常策略：IO 类 500ms、其余 1000ms 后继续监听（fail-closed 但仍监听）
                    let delay = match e.kind() {
                        ErrorKind::TimedOut | ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe
                        | ErrorKind::NotConnected | ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted => Duration::from_millis(500),
                        _ => Duration::from_secs(1),
                    };
                    warn!("管道监听错误: {e}；{delay:?} 后继续监听");
                    sleep(delay).await;
                }
            }
        }
    }

    /// ACL 建流（§3.9 CreateServerStream）：DACL 烧死于创建时刻；构建失败绝不回退开放默认
    async fn accept_connected(&self) -> Result<NamedPipeServer> {
        let mut sec = build_pipe_dacl_sd(&self.allowed_sids)
            .map_err(|e| { error!("管道 DACL 构建失败，本轮不建流（不回退开放默认）: {e}"); e })?;
        loop {
            let mut opts = ServerOptions::new();
            opts.first_pipe_instance(true)
                .access_inbound(true)
                .access_outbound(true)
                .max_instances(1);
            let mut pipe = unsafe { opts.create_with_security_attributes_raw(PIPE_NAME, sec.raw()) }?;
            match timeout(ACCEPT_WINDOW, pipe.connect()).await {
                Ok(Ok(())) => return Ok(pipe),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    // 2s 无接入：销毁实例重新挂台，不给未授权进程留占坑窗口
                    debug!("接入窗 {ACCEPT_WINDOW:?} 内无客户端，重建管道实例");
                }
            }
        }
    }

    /// 运行时 SID 校验：纵深防御（管道 DACL 之外的第二道闸）
    fn client_sid_authorized(pipe: &NamedPipeServer, allowed: &[Vec<u8>]) -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows::core::PWSTR;
        use windows::Win32::Foundation::{CloseHandle, HANDLE, LocalFree, HLOCAL};
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_ACCESS_MASK};
        use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
        use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS};

        const PROCESS_QUERY_LIMITED_INFORMATION: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0400);
        const TOKEN_QUERY: TOKEN_ACCESS_MASK = TOKEN_ACCESS_MASK(0x0008);

        fn close(h: HANDLE) {
            unsafe {
                if CloseHandle(h).is_err() {
                    warn!("CloseHandle 失败（句柄可能已失效）");
                }
            }
        }

        unsafe {
            let handle = HANDLE(pipe.as_raw_handle() as _);
            let mut pid = 0u32;
            if GetNamedPipeClientProcessId(handle, &mut pid).is_err() || pid == 0 {
                warn!("GetNamedPipeClientProcessId 失败，无法识别客户端进程");
                return false;
            }
            let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                warn!("打开客户端进程 (pid={pid}) 失败");
                return false;
            };
            let mut token = HANDLE::default();
            if OpenProcessToken(proc_handle, TOKEN_QUERY, &mut token).is_err() {
                close(proc_handle);
                return false;
            }
            let mut size = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
            let mut buf = vec![0u8; size as usize];
            let ok = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut _), size, &mut size).is_ok();
            close(token);
            if !ok {
                close(proc_handle);
                return false;
            }
            let sid = (&*(buf.as_ptr() as *const windows::Win32::Security::TOKEN_USER)).User.Sid;
            let mut sid_str = PWSTR::null();
            if ConvertSidToStringSidW(sid, &mut sid_str).is_err() {
                close(proc_handle);
                return false;
            }
            // 元素计数止于 NUL——乘 2 会越读吞进堆垃圾，令白名单比对静默失效
            let len = (0..).take_while(|&i| *sid_str.0.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str.0, len));
            let _ = LocalFree(Some(HLOCAL(sid_str.0 as *mut _)));
            close(proc_handle);
            check_client_sid(s.as_bytes(), allowed)
        }
    }

    async fn handle_connection(mut pipe: NamedPipeServer) {
        loop {
            let frame = match read_frame_async(&mut pipe).await {
                Ok(Some(f)) => f,
                Ok(None) => break,
                // 超长头在读载荷前就返回：帧边界已被打穿，只能断线重来
                Err(e) if e.to_string().contains("载荷超过 1MB") => {
                    warn!("帧声明超过 1MB 上限，断开: {e}");
                    break;
                }
                Err(e) if e.kind() == ErrorKind::InvalidData => {
                    // §3.9 坏帧不掀全管：载荷已被完整消费，帧边界完好，warn 后继续读下一帧
                    warn!("坏帧丢弃: {e}");
                    continue;
                }
                Err(e) => {
                    warn!("帧读取错误，断开连接: {e}");
                    break;
                }
            };
            match frame.payload {
                Payload::RelativeMouseBatch(batch) => {
                    let last_seq = if batch.count == 0 {
                        batch.first_seq
                    } else {
                        batch.first_seq + batch.count as u64 - 1
                    };
                    let result = Frame {
                        frame_type: FrameType::RelativeMouseResult,
                        payload: Payload::RelativeMouseResult(RelativeMouseResult {
                            last_seq,
                            handled: true,
                        }),
                    };
                    if let Err(e) = write_frame_async(&mut pipe, &result).await {
                        warn!("写回放结果失败: {e}");
                        break;
                    }
                }
                Payload::Utf8Json(_) => {
                    let response = Frame {
                        frame_type: FrameType::Utf8Json,
                        payload: Payload::Utf8Json(r#"{"status":"ok"}"#.into()),
                    };
                    if let Err(e) = write_frame_async(&mut pipe, &response).await {
                        warn!("写 JSON 响应失败: {e}");
                        break;
                    }
                }
                other => warn!("数据期收到意外帧，忽略: {other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// 纯"哑客户端"：只按 §3.8 线路格式说话，验证 server 侧行为
    async fn dumb_client(mut c: impl AsyncRead + AsyncWrite + Unpin, nonce: [u8; NONCE_LEN]) -> Option<u8> {
        let f = Frame { frame_type: FrameType::Handshake, payload: Payload::Handshake(nonce) };
        if write_frame_async(&mut c, &f).await.is_err() {
            return None;
        }
        match timeout(HANDSHAKE_TIMEOUT, read_frame_async(&mut c)).await {
            Ok(Ok(Some(fr))) => match fr.payload {
                Payload::HandshakeAck(v) => Some(v),
                _ => None,
            },
            _ => None,
        }
    }

    #[tokio::test]
    async fn handshake_accepts_correct_nonce_and_sends_one_byte_verdict() {
        let nonce = [7u8; NONCE_LEN];
        let (mut s, mut c) = duplex(4096);
        let (ok, verdict) =
            tokio::join!(verify_handshake(&mut s, Some(nonce)), dumb_client(&mut c, nonce));
        assert!(ok, "正确 nonce 必须接受，且不得互等死锁");
        assert_eq!(verdict, Some(HANDSHAKE_ACCEPTED));
    }

    #[tokio::test]
    async fn handshake_rejects_wrong_nonce_with_verdict() {
        let (mut s, mut c) = duplex(4096);
        let (ok, verdict) = tokio::join!(
            verify_handshake(&mut s, Some([1u8; NONCE_LEN])),
            dumb_client(&mut c, [2u8; NONCE_LEN])
        );
        assert!(!ok);
        assert_eq!(verdict, Some(HANDSHAKE_REJECTED), "错误 nonce 必须收到显式 0x00 而非断线");
    }

    #[tokio::test]
    async fn handshake_fail_closed_without_expected_nonce() {
        let (mut s, mut c) = duplex(4096);
        let nonce = [3u8; NONCE_LEN];
        let (ok, verdict) = tokio::join!(verify_handshake(&mut s, None), dumb_client(&mut c, nonce));
        assert!(!ok, "未配置期望 nonce 必须 fail-closed");
        assert_eq!(verdict, Some(HANDSHAKE_REJECTED), "拒绝也要发 verdict");
    }

    #[tokio::test]
    async fn handshake_rejects_non_handshake_frame() {
        let (mut s, mut c) = duplex(4096);
        let client = async move {
            let f = Frame { frame_type: FrameType::Utf8Json, payload: Payload::Utf8Json("x".into()) };
            if write_frame_async(&mut c, &f).await.is_err() {
                return None;
            }
            match timeout(HANDSHAKE_TIMEOUT, read_frame_async(&mut c)).await {
                Ok(Ok(Some(fr))) => match fr.payload {
                    Payload::HandshakeAck(v) => Some(v),
                    _ => None,
                },
                _ => None,
            }
        };
        let (ok, verdict) =
            tokio::join!(verify_handshake(&mut s, Some([5u8; NONCE_LEN])), client);
        assert!(!ok);
        assert_eq!(verdict, Some(HANDSHAKE_REJECTED));
    }

    #[tokio::test]
    async fn handshake_silent_client_times_out_without_deadlock() {
        // 真实等满 5s：同时证明 HANDSHAKE_TIMEOUT 常量被真实接线、静默客户端不会永久卡死单管道实例
        let (mut s, _c) = duplex(4096);
        let ok = verify_handshake(&mut s, Some([9u8; NONCE_LEN])).await;
        assert!(!ok, "静默客户端 5s 超时后必须拒绝而非永久卡死单管道实例");
    }
}
