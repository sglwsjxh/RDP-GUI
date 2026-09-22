// AGPL-3.0 许可证

use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() {
    println!("运行帧协议自测...");

    test_frame_roundtrip_json();
    println!("块 1/14: JSON 往返 通过");

    test_frame_roundtrip_batch();
    println!("块 2/14: 批量鼠标 往返 通过");

    test_frame_roundtrip_result();
    println!("块 3/14: 鼠标结果 往返 通过");

    test_frame_roundtrip_handshake();
    println!("块 14/14: 握手 往返 通过");

    test_frame_roundtrip_handshake_ack();
    println!("块 14b/14: 握手确认 往返 通过");

    #[cfg(debug_assertions)]
    {
        test_guard_payload_too_large();
        println!("守卫: 载荷过大 panic 通过");

        test_guard_count_too_large();
        println!("守卫: 事件数过大 panic 通过");
    }

    test_const_eq();
    println!("恒定时间比较 通过");

    test_eof_returns_none();
    println!("EOF 返回 None 通过");

    test_bad_frame_drop_continue();
    println!("坏帧丢弃继续 通过");

    test_struct_sizes();
    println!("块 5/14: 结构体尺寸 通过");

    test_env_check_count();
    println!("块 6/14: 环境检查条数 通过");

    test_child_sessions_probe();
    println!("块 7/14: 子会话开关探测 通过");

    test_rdp_port_probe();
    println!("块 8/14: RDP 端口探测 通过");

    test_rdp_listener_probe();
    println!("块 9/14: RDP 监听探测 通过");

    test_rdp_wrapper_probe();
    println!("块 10/14: RDP Wrapper 探测 通过");

    test_termsrv_version_probe();
    println!("块 11/14: termsrv 版本探测 通过");

    test_firewall_rule_string();
    println!("块 12/14: 防火墙规则字符串 通过");

    test_apply_fixes_wrapper_hook_skip();
    println!("块 13/14: 修复保留 Wrapper hook SKIP: 需管理员/真机");

    test_agent_smoke();
    println!("Agent 冒烟 通过（无 nonce 时 fail-closed）");

    test_ui_smoke_skip();
    println!("块 14c/14: UI 冒烟 SKIP: 需交互桌面/真机");

    test_e2e_rdp_skip();
    println!("块 14d/14: E2E RDP SKIP: 需真机 RDP 环境");

    println!("全部自测通过");
}

fn test_frame_roundtrip_json() {
    use akispace::ipc::frame::{Frame, FrameType, Payload};
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

fn test_frame_roundtrip_batch() {
    use akispace::ipc::frame::{Frame, FrameType, Payload, RelativeMouseBatch, MouseEvent};
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

fn test_frame_roundtrip_result() {
    use akispace::ipc::frame::{Frame, FrameType, Payload, RelativeMouseResult};
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

fn test_frame_roundtrip_handshake() {
    use akispace::ipc::frame::{Frame, FrameType, Payload, NONCE_LEN, const_eq};
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

fn test_frame_roundtrip_handshake_ack() {
    use akispace::ipc::frame::{Frame, FrameType, Payload, HEADER_LEN, HANDSHAKE_ACCEPTED};
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

fn test_guard_payload_too_large() {
    use akispace::ipc::frame::{guard_payload_len, MAX_PAYLOAD};
    let result = std::panic::catch_unwind(|| {
        guard_payload_len(MAX_PAYLOAD as usize + 1);
    });
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.downcast_ref::<&str>().unwrap();
    assert!(msg.contains("载荷超过 1MB"));
}

fn test_guard_count_too_large() {
    use akispace::ipc::frame::guard_count;
    let result = std::panic::catch_unwind(|| {
        guard_count(65536);
    });
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.downcast_ref::<&str>().unwrap();
    assert!(msg.contains("事件数超过 65535"));
}

fn test_const_eq() {
    use akispace::ipc::frame::const_eq;
    assert!(const_eq(b"abc", b"abc"));
    assert!(!const_eq(b"abc", b"abd"));
    assert!(!const_eq(b"abc", b"abcd"));
}

fn test_eof_returns_none() {
    use akispace::ipc::frame::Frame;
    let empty: &[u8] = &[];
    let result = Frame::decode(empty).unwrap();
    assert!(result.is_none());
}

fn test_bad_frame_drop_continue() {
    use akispace::ipc::frame::{Frame, FrameReader, FrameType, Payload};
    let mut bad = vec![100u8, 0, 0, 0, 0x99];
    bad.extend_from_slice(&[0u8; 100]);
    let good = Frame {
        frame_type: FrameType::Utf8Json,
        payload: Payload::Utf8Json("ok".into()),
    }.encode();
    let mut combined = bad;
    combined.extend_from_slice(&good);
    let mut reader = FrameReader::new(&combined[..]);
    let first = reader.read_frame().unwrap();
    assert!(first.is_none());
    let second = reader.read_frame().unwrap();
    assert!(second.is_some());
}

fn test_struct_sizes() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, MOUSEINPUT};
    use windows::Win32::UI::Input::{RAWINPUT, RAWINPUTHEADER};

    assert_eq!(std::mem::size_of::<INPUT>(), 40);
    assert_eq!(std::mem::size_of::<MOUSEINPUT>(), 32);
    assert_eq!(std::mem::size_of::<RAWINPUT>(), 48);
    assert_eq!(std::mem::size_of::<RAWINPUTHEADER>(), 24);
}

fn test_env_check_count() {
    let results = akispace::environment::run_all_checks();
    assert_eq!(results.len(), 10);
}

fn test_child_sessions_probe() {
    let r = akispace::session::is_child_sessions_enabled();
    match r {
        Ok(v) => println!("  子会话开关: 启用={}", v),
        Err(e) => println!("  子会话开关: 查询失败（家庭版预期内）: {}", e),
    }
}

fn test_rdp_port_probe() {
    let port = akispace::session::get_configured_rdp_port();
    assert!(port != 0, "RDP 端口为 0 非法: {}", port);
    println!("  配置 RDP 端口: {}", port);
}

fn test_rdp_listener_probe() {
    let port = akispace::session::get_configured_rdp_port();
    let active = akispace::session::is_rdp_listener_active(port);
    println!("  端口 {} 监听: {}", port, active);
}

fn test_rdp_wrapper_probe() {
    let installed = akispace::session::is_rdp_wrapper_installed();
    println!("  RDP Wrapper 已安装: {}", installed);
}

fn test_termsrv_version_probe() {
    let r = akispace::session::get_termsrv_version();
    match r {
        Ok(v) => {
            assert!(!v.is_empty(), "版本字符串为空");
            println!("  termsrv 版本: {}", v);
        }
        Err(e) => println!("  termsrv 版本: 读取失败（非标准系统预期内）: {}", e),
    }
}

fn test_firewall_rule_string() {
    use akispace::environment::CheckItem;
    let results = akispace::environment::run_all_checks();
    let firewall = results.iter().find(|r| r.item == CheckItem::FirewallLoopbackRule);
    assert!(firewall.is_some(), "缺少 FirewallLoopbackRule 检查项");
    let f = firewall.unwrap();
    assert!(f.detail.contains("规则"), "防火墙检查详情异常: {}", f.detail);
    println!("  防火墙规则检查: {} / {}", f.detail, f.passed);
}

// ponytail: 需管理员/真机，离线 SKIP；C# 原块 13 断言 ApplyAllFixes(false) 保留 wrapper hook
fn test_apply_fixes_wrapper_hook_skip() {
}

fn test_agent_smoke() {
    let mut child = Command::new("cargo")
        .args(["run", "--bin", "akispace-agent"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("启动 agent 失败");

    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    let output = loop {
        if let Ok(Some(status)) = child.try_wait() {
            let output = child.wait_with_output().expect("等待输出失败");
            break output;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            panic!("agent 启动超时");
        }
        thread::sleep(Duration::from_millis(100));
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // agent 无 nonce 文件时 fail-closed：非零退出 + stderr 含 "nonce"（实测 stderr 为
    // `Error: Custom { kind: NotFound, error: "nonce 文件不存在" }`）
    assert!(
        !output.status.success(),
        "agent 无 nonce 应 fail-closed 非零退出"
    );
    assert!(
        stderr.contains("nonce") || stdout.contains("nonce"),
        "agent 无 nonce 应拒绝运行（fail-closed），实际输出: stdout={stdout}, stderr={stderr}"
    );
}

fn test_ui_smoke_skip() {
    // SKIP: 需交互桌面/真机环境，CI 无显示服务器
    // 对应 C# 原版 UI 冒烟测试（窗口创建、渲染、输入处理）
}

fn test_e2e_rdp_skip() {
    // SKIP: 需真机 RDP 环境（子会话/标准 RDP）、网络、凭据
    // 对应 C# 原版端到端 RDP 连接测试
}