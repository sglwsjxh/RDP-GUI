// AGPL-3.0 许可证

use std::io::{Read, Write, Error, ErrorKind, Result};
use std::sync::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_PAYLOAD: u32 = 1 << 20;
pub const NONCE_LEN: usize = 32;
pub const HEADER_LEN: usize = 5;

/// 握手裁决（§3.8 基线：HandshakeAck 载荷是 1 字节 verdict，非 32 字节 nonce）
pub const HANDSHAKE_ACCEPTED: u8 = 0x01;
pub const HANDSHAKE_REJECTED: u8 = 0x00;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Utf8Json = 1,
    RelativeMouseBatch = 2,
    RelativeMouseResult = 3,
    Handshake = 4,
    HandshakeAck = 5,
}

impl TryFrom<u8> for FrameType {
    type Error = Error;
    fn try_from(v: u8) -> Result<Self> {
        match v {
            1 => Ok(FrameType::Utf8Json),
            2 => Ok(FrameType::RelativeMouseBatch),
            3 => Ok(FrameType::RelativeMouseResult),
            4 => Ok(FrameType::Handshake),
            5 => Ok(FrameType::HandshakeAck),
            _ => Err(Error::new(ErrorKind::InvalidData, "未知帧类型")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelativeMouseBatch {
    pub count: u16,
    pub first_seq: u64,
    pub base_ticks: i64,
    pub events: Vec<MouseEvent>,
}

#[derive(Debug, Clone)]
pub struct MouseEvent {
    pub dx: i32,
    pub dy: i32,
    pub rel_ticks: i64,
}

#[derive(Debug, Clone)]
pub struct RelativeMouseResult {
    pub last_seq: u64,
    pub handled: bool,
}

#[derive(Debug, Clone)]
pub enum Payload {
    Utf8Json(String),
    RelativeMouseBatch(RelativeMouseBatch),
    RelativeMouseResult(RelativeMouseResult),
    Handshake([u8; NONCE_LEN]),
    /// 服→客 握手裁决：1 字节，HANDSHAKE_ACCEPTED/HANDSHAKE_REJECTED
    HandshakeAck(u8),
}

pub struct Frame {
    pub frame_type: FrameType,
    pub payload: Payload,
}

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let payload_bytes = self.payload.encode();
        guard_payload_len(payload_bytes.len());
        let mut buf = Vec::with_capacity(HEADER_LEN + payload_bytes.len());
        buf.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes());
        buf.push(self.frame_type as u8);
        buf.extend_from_slice(&payload_bytes);
        buf
    }

    pub fn decode(mut r: impl Read) -> Result<Option<Self>> {
        let mut header = [0u8; HEADER_LEN];
        match r.read_exact(&mut header) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        if len > MAX_PAYLOAD as usize {
            return Err(Error::new(ErrorKind::InvalidData, "载荷超过 1MB"));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        let frame_type = FrameType::try_from(header[4])?;
        let payload = Payload::decode(frame_type, &payload)?;
        Ok(Some(Frame { frame_type, payload }))
    }
}

impl Payload {
    fn encode(&self) -> Vec<u8> {
        match self {
            Payload::Utf8Json(s) => s.as_bytes().to_vec(),
            Payload::RelativeMouseBatch(b) => {
                let mut buf = Vec::with_capacity(4 + 8 + 8 + b.events.len() * 16);
                buf.extend_from_slice(&b.count.to_le_bytes());
                buf.extend_from_slice(&b.first_seq.to_le_bytes());
                buf.extend_from_slice(&b.base_ticks.to_le_bytes());
                for e in &b.events {
                    buf.extend_from_slice(&e.dx.to_le_bytes());
                    buf.extend_from_slice(&e.dy.to_le_bytes());
                    buf.extend_from_slice(&e.rel_ticks.to_le_bytes());
                }
                buf
            }
            Payload::RelativeMouseResult(r) => {
                let mut buf = Vec::with_capacity(8 + 1);
                buf.extend_from_slice(&r.last_seq.to_le_bytes());
                buf.push(r.handled as u8);
                buf
            }
            Payload::Handshake(n) => n.to_vec(),
            Payload::HandshakeAck(v) => vec![*v],
        }
    }

    pub fn decode(frame_type: FrameType, data: &[u8]) -> Result<Self> {
        match frame_type {
            FrameType::Utf8Json => {
                let s = String::from_utf8(data.to_vec())
                    .map_err(|_| Error::new(ErrorKind::InvalidData, "UTF-8 解码失败"))?;
                Ok(Payload::Utf8Json(s))
            }
            FrameType::RelativeMouseBatch => {
                if data.len() < 18 {
                    return Err(Error::new(ErrorKind::InvalidData, "批载荷过短"));
                }
                let count = u16::from_le_bytes([data[0], data[1]]) as usize;
                if count > 65535 {
                    return Err(Error::new(ErrorKind::InvalidData, "事件数超过 65535"));
                }
                let expected = 18 + count * 16;
                if data.len() != expected {
                    return Err(Error::new(ErrorKind::InvalidData, "批载荷长度不匹配"));
                }
                let first_seq = u64::from_le_bytes(data[2..10].try_into().unwrap());
                let base_ticks = i64::from_le_bytes(data[10..18].try_into().unwrap());
                let mut events = Vec::with_capacity(count);
                let mut off = 18;
                for _ in 0..count {
                    let dx = i32::from_le_bytes(data[off..off+4].try_into().unwrap());
                    let dy = i32::from_le_bytes(data[off+4..off+8].try_into().unwrap());
                    let rel_ticks = i64::from_le_bytes(data[off+8..off+16].try_into().unwrap());
                    events.push(MouseEvent { dx, dy, rel_ticks });
                    off += 16;
                }
                Ok(Payload::RelativeMouseBatch(RelativeMouseBatch { count: count as u16, first_seq, base_ticks, events }))
            }
            FrameType::RelativeMouseResult => {
                if data.len() != 9 {
                    return Err(Error::new(ErrorKind::InvalidData, "结果载荷长度错误"));
                }
                let last_seq = u64::from_le_bytes(data[0..8].try_into().unwrap());
                let handled = data[8] != 0;
                Ok(Payload::RelativeMouseResult(RelativeMouseResult { last_seq, handled }))
            }
            FrameType::Handshake => {
                if data.len() != NONCE_LEN {
                    return Err(Error::new(ErrorKind::InvalidData, "Nonce 长度错误"));
                }
                let mut nonce = [0u8; NONCE_LEN];
                nonce.copy_from_slice(data);
                Ok(Payload::Handshake(nonce))
            }
            FrameType::HandshakeAck => {
                if data.len() != 1 {
                    return Err(Error::new(ErrorKind::InvalidData, "verdict 长度错误"));
                }
                Ok(Payload::HandshakeAck(data[0]))
            }
        }
    }
}

pub fn guard_payload_len(len: usize) {
    if len > MAX_PAYLOAD as usize {
        panic!("载荷超过 1MB");
    }
}

pub fn guard_count(count: usize) {
    if count > 65535 {
        panic!("事件数超过 65535");
    }
}

pub struct FrameWriter<W: Write> {
    inner: Mutex<W>,
}

impl<W: Write> FrameWriter<W> {
    pub fn new(w: W) -> Self {
        Self { inner: Mutex::new(w) }
    }

    pub fn write_frame(&self, frame: &Frame) -> Result<()> {
        let mut w = self.inner.lock().unwrap();
        let bytes = frame.encode();
        w.write_all(&bytes)
    }
}

pub struct FrameReader<R: Read> {
    inner: R,
}

impl<R: Read> FrameReader<R> {
    pub fn new(r: R) -> Self {
        Self { inner: r }
    }

    pub fn read_frame(&mut self) -> Result<Option<Frame>> {
        loop {
            match Frame::decode(&mut self.inner) {
                Ok(Some(frame)) => return Ok(Some(frame)),
                Ok(None) => return Ok(None),
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("未知帧类型")
                        || msg.contains("批载荷长度不匹配")
                        || msg.contains("结果载荷长度错误")
                        || msg.contains("Nonce 长度错误")
                        || msg.contains("verdict 长度错误")
                        || msg.contains("UTF-8 解码失败")
                        || msg.contains("批载荷过短") {
                        return Ok(None);
                    }
                    let mut byte = [0u8; 1];
                    match self.inner.read_exact(&mut byte) {
                        Ok(_) => continue,
                        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
                        Err(e) => return Err(e),
                    }
                }
            }
        }
    }
}

pub fn const_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 异步读一帧（tokio 管道与 duplex 测试共用）。
/// 取消安全前提：调用方超时后必须丢弃整条流——握手失败即断开重连，复用半截读取不可。
pub async fn read_frame_async<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Frame>> {
    let mut header = [0u8; HEADER_LEN];
    match r.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    if len > MAX_PAYLOAD as usize {
        return Err(Error::new(ErrorKind::InvalidData, "载荷超过 1MB"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    let frame_type = FrameType::try_from(header[4])?;
    let payload = Payload::decode(frame_type, &payload)?;
    Ok(Some(Frame { frame_type, payload }))
}

/// 异步写一帧（写透，无用户态缓冲，等价 C# 每帧 Flush）
pub async fn write_frame_async<W: AsyncWrite + Unpin>(w: &mut W, frame: &Frame) -> Result<()> {
    w.write_all(&frame.encode()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip_json() {
        let frame = Frame {
            frame_type: FrameType::Utf8Json,
            payload: Payload::Utf8Json(r#"{"cmd":"ping"}"#.into()),
        };
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded[..]).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FrameType::Utf8Json);
        match decoded.payload {
            Payload::Utf8Json(s) => assert_eq!(s, r#"{"cmd":"ping"}"#),
            _ => panic!("类型不匹配"),
        }
    }

    #[test]
    fn test_frame_roundtrip_batch() {
        let frame = Frame {
            frame_type: FrameType::RelativeMouseBatch,
            payload: Payload::RelativeMouseBatch(RelativeMouseBatch {
                count: 2,
                first_seq: 100,
                base_ticks: 1000,
                events: vec![
                    MouseEvent { dx: 10, dy: 20, rel_ticks: 10 },
                    MouseEvent { dx: -5, dy: 15, rel_ticks: 20 },
                ],
            }),
        };
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded[..]).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FrameType::RelativeMouseBatch);
        match decoded.payload {
            Payload::RelativeMouseBatch(b) => {
                assert_eq!(b.count, 2);
                assert_eq!(b.first_seq, 100);
                assert_eq!(b.base_ticks, 1000);
                assert_eq!(b.events.len(), 2);
            }
            _ => panic!("类型不匹配"),
        }
    }

    #[test]
    fn test_frame_roundtrip_result() {
        let frame = Frame {
            frame_type: FrameType::RelativeMouseResult,
            payload: Payload::RelativeMouseResult(RelativeMouseResult {
                last_seq: 101,
                handled: true,
            }),
        };
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded[..]).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FrameType::RelativeMouseResult);
        match decoded.payload {
            Payload::RelativeMouseResult(r) => {
                assert_eq!(r.last_seq, 101);
                assert!(r.handled);
            }
            _ => panic!("类型不匹配"),
        }
    }

    #[test]
    fn test_frame_roundtrip_handshake() {
        let nonce = [42u8; NONCE_LEN];
        let frame = Frame {
            frame_type: FrameType::Handshake,
            payload: Payload::Handshake(nonce),
        };
        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded[..]).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FrameType::Handshake);
        match decoded.payload {
            Payload::Handshake(n) => assert!(const_eq(&n, &nonce)),
            _ => panic!("类型不匹配"),
        }
    }

    #[test]
    fn test_frame_roundtrip_handshake_ack() {
        let frame = Frame {
            frame_type: FrameType::HandshakeAck,
            payload: Payload::HandshakeAck(HANDSHAKE_ACCEPTED),
        };
        let encoded = frame.encode();
        assert_eq!(encoded.len(), HEADER_LEN + 1, "§3.8：verdict 载荷恒 1 字节");
        let decoded = Frame::decode(&encoded[..]).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FrameType::HandshakeAck);
        match decoded.payload {
            Payload::HandshakeAck(v) => assert_eq!(v, HANDSHAKE_ACCEPTED),
            _ => panic!("类型不匹配"),
        }
    }

    #[test]
    fn test_handshake_ack_rejects_nonce_len() {
        let err = Payload::decode(FrameType::HandshakeAck, &[7u8; NONCE_LEN]).unwrap_err();
        assert!(err.to_string().contains("verdict 长度错误"));
    }

    #[test]
    #[should_panic(expected = "载荷超过 1MB")]
    fn test_guard_payload_too_large() {
        guard_payload_len(MAX_PAYLOAD as usize + 1);
    }

    #[test]
    #[should_panic(expected = "事件数超过 65535")]
    fn test_guard_count_too_large() {
        guard_count(65536);
    }

    #[test]
    fn test_const_eq() {
        assert!(const_eq(b"abc", b"abc"));
        assert!(!const_eq(b"abc", b"abd"));
        assert!(!const_eq(b"abc", b"abcd"));
    }

    #[test]
    fn test_eof_returns_none() {
        let empty: &[u8] = &[];
        let result = Frame::decode(empty).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_bad_frame_drop_continue() {
        use std::io::Cursor;
        let mut bad = vec![100u8, 0, 0, 0, 0x99];
        bad.extend_from_slice(&[0u8; 100]);
        let good = Frame {
            frame_type: FrameType::Utf8Json,
            payload: Payload::Utf8Json("ok".into()),
        }.encode();
        let mut combined = bad;
        combined.extend_from_slice(&good);
        let mut reader = FrameReader::new(Cursor::new(combined));
        let first = reader.read_frame().unwrap();
        assert!(first.is_none());
        let second = reader.read_frame().unwrap();
        assert!(second.is_some());
    }
}