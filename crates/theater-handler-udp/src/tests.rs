//! Conformance suite for the UDP handler. Tests the real building blocks
//! (bind_udp, recv_from_bounded, udp_recv_loop, join_or_leave, the permission
//! checks, the interface) against live loopback `tokio::net::UdpSocket` peers.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use theater::actor::handle::ActorHandle;
use theater::actor::types::ActorOperation;

// ---- helpers ---------------------------------------------------------------

/// Shared record of `(peer, data)` pairs an active-mode delivery pushed.
type Recorded = Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

/// A mock actor: drains the op channel and records each `on-datagram` as
/// (peer, data). If `respond` is true it answers each call so the delivery task
/// proceeds; if false it never answers (and does not drain), modeling a wedged
/// actor so active-mode backpressure engages.
fn mock_actor(respond: bool) -> (ActorHandle, Recorded) {
    let (op_tx, mut op_rx) = tokio::sync::mpsc::channel::<ActorOperation>(4);
    let (info_tx, _info_rx) = tokio::sync::mpsc::channel(4);
    let (ctrl_tx, _ctrl_rx) = tokio::sync::mpsc::channel(4);
    let handle = ActorHandle::new(op_tx, info_tx, ctrl_tx);
    let got = Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    if respond {
        let got_t = got.clone();
        tokio::spawn(async move {
            while let Some(ActorOperation::CallFunctionPack {
                name,
                params,
                response_tx,
            }) = op_rx.recv().await
            {
                if name == "theater:simple/udp-client.on-datagram" {
                    if let Ok(Value::Tuple(v)) = theater::pack_bridge::decode_value(&params) {
                        let peer = match v.get(1) {
                            Some(Value::String(s)) => s.clone(),
                            _ => String::new(),
                        };
                        let data: Vec<u8> = match v.get(2) {
                            Some(Value::List { items, .. }) => items
                                .iter()
                                .filter_map(|x| if let Value::U8(b) = x { Some(*b) } else { None })
                                .collect(),
                            _ => Vec::new(),
                        };
                        got_t.lock().unwrap().push((peer, data));
                    }
                }
                let _ = response_tx.send(Ok(vec![]));
            }
        });
    } else {
        // Hold op_rx alive but never drain → op_tx (bounded) fills → the
        // delivery task blocks on call_function → the recv loop drops.
        std::mem::forget(op_rx);
    }
    (handle, got)
}

async fn poll_until<F: Fn() -> bool>(f: F, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    f()
}

// ---- tests -----------------------------------------------------------------

#[tokio::test]
async fn bind_and_loopback_roundtrip() {
    let a = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let b = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let a_addr = a.local_addr().unwrap();
    let b_addr = b.local_addr().unwrap();
    b.send_to(b"hello", a_addr).await.unwrap();
    let (data, peer) = recv_from_bounded(
        &a,
        1500,
        Some(Duration::from_secs(2)),
        &CancellationToken::new(),
    )
    .await
    .expect("should receive");
    assert_eq!(data, b"hello");
    assert_eq!(peer, b_addr.to_string());
}

#[tokio::test]
async fn recv_from_bounded_times_out_on_silent_socket() {
    let a = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let t0 = std::time::Instant::now();
    let err = recv_from_bounded(
        &a,
        1500,
        Some(Duration::from_millis(150)),
        &CancellationToken::new(),
    )
    .await
    .expect_err("silent socket must time out");
    assert!(
        t0.elapsed() < Duration::from_secs(2),
        "must return promptly"
    );
    match err {
        Value::String(s) => assert!(
            s.starts_with("timeout:"),
            "want timeout-prefixed, got {s:?}"
        ),
        other => panic!("want Value::String, got {other:?}"),
    }
}

#[tokio::test]
async fn connect_filters_to_peer() {
    let h = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let peer = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let other = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    let h_addr = h.local_addr().unwrap();
    // Connect h to `peer`: the OS now filters recv to that peer only.
    h.connect(peer.local_addr().unwrap()).await.unwrap();

    // A datagram from `other` must NOT be delivered (connected filter).
    other.send_to(b"nope", h_addr).await.unwrap();
    let r = recv_from_bounded(
        &h,
        1500,
        Some(Duration::from_millis(200)),
        &CancellationToken::new(),
    )
    .await;
    assert!(
        r.is_err(),
        "datagram from a non-connected peer must not arrive"
    );

    // A datagram from `peer` IS delivered.
    peer.send_to(b"yes", h_addr).await.unwrap();
    let (data, _) = recv_from_bounded(
        &h,
        1500,
        Some(Duration::from_secs(2)),
        &CancellationToken::new(),
    )
    .await
    .expect("datagram from the connected peer must arrive");
    assert_eq!(data, b"yes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_mode_pushes_on_datagram() {
    let h = Arc::new(bind_udp("127.0.0.1:0".parse().unwrap()).unwrap());
    let h_addr = h.local_addr().unwrap();
    let (actor, got) = mock_actor(true);
    let cancel = CancellationToken::new();
    let dropped = Arc::new(AtomicU64::new(0));
    let loop_h = {
        let (h, dropped, cancel) = (h.clone(), dropped.clone(), cancel.clone());
        tokio::spawn(async move { udp_recv_loop(1, h, actor, dropped, false, cancel).await })
    };

    let peer = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    peer.send_to(b"ping", h_addr).await.unwrap();

    let ok = poll_until(|| !got.lock().unwrap().is_empty(), Duration::from_secs(3)).await;
    assert!(ok, "on-datagram should have been delivered");
    let rec = got.lock().unwrap().clone();
    assert_eq!(rec[0].1, b"ping");
    assert_eq!(rec[0].0, peer.local_addr().unwrap().to_string());

    cancel.cancel();
    let _ = loop_h.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_mode_bounded_drop_under_flood() {
    let h = Arc::new(bind_udp("127.0.0.1:0".parse().unwrap()).unwrap());
    let h_addr = h.local_addr().unwrap();
    // Wedged actor: delivery blocks → the bounded channel fills → drops.
    let (actor, _got) = mock_actor(false);
    let cancel = CancellationToken::new();
    let dropped = Arc::new(AtomicU64::new(0));
    let loop_h = {
        let (h, dropped, cancel) = (h.clone(), dropped.clone(), cancel.clone());
        tokio::spawn(async move { udp_recv_loop(2, h, actor, dropped, false, cancel).await })
    };

    // Flood well past ACTIVE_DELIVERY_CAP.
    let peer = bind_udp("127.0.0.1:0".parse().unwrap()).unwrap();
    for _ in 0..(ACTIVE_DELIVERY_CAP * 4) {
        let _ = peer.send_to(b"flood", h_addr).await;
    }

    let dropped_some = poll_until(
        || dropped.load(Ordering::Relaxed) > 0,
        Duration::from_secs(5),
    )
    .await;
    assert!(
        dropped_some,
        "bounded active delivery must DROP under a flood a wedged actor can't drain (got {})",
        dropped.load(Ordering::Relaxed)
    );
    // The wedged mock never answers the in-flight call_function, so the loop's
    // delivery task can't drain — cancel then ABORT (don't await, which would
    // block on that stuck delivery). In production the actor's channels close on
    // teardown, so call_function errors out and the loop exits cleanly.
    cancel.cancel();
    loop_h.abort();
}

#[tokio::test]
async fn dualstack_v6_roundtrip() {
    let a = match bind_udp("[::1]:0".parse().unwrap()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: no IPv6 loopback here ({e})");
            return;
        }
    };
    let b = bind_udp("[::1]:0".parse().unwrap()).unwrap();
    let a_addr = a.local_addr().unwrap();
    b.send_to(b"v6", a_addr).await.unwrap();
    let (data, _) = recv_from_bounded(
        &a,
        1500,
        Some(Duration::from_secs(2)),
        &CancellationToken::new(),
    )
    .await
    .expect("v6 roundtrip");
    assert_eq!(data, b"v6");
}

#[tokio::test]
async fn multicast_v4_join_send_recv() {
    // Best-effort: multicast often doesn't route in CI/containers — skip (not
    // fail) if join or delivery doesn't work here.
    let group = "239.255.77.78";
    let recv = match bind_udp("0.0.0.0:0".parse().unwrap()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP multicast: bind failed ({e})");
            return;
        }
    };
    let port = recv.local_addr().unwrap().port();
    if let Err(e) = recv.set_multicast_loop_v4(true) {
        eprintln!("SKIP multicast: set loop failed ({e:?})");
        return;
    }
    if let Err(e) = join_or_leave(&recv, group, "0.0.0.0", true) {
        eprintln!("SKIP multicast: join failed ({e:?})");
        return;
    }
    let sender = bind_udp("0.0.0.0:0".parse().unwrap()).unwrap();
    let _ = sender.set_multicast_loop_v4(true);
    let group_addr: SocketAddr = format!("{group}:{port}").parse().unwrap();
    if sender.send_to(b"mcast", group_addr).await.is_err() {
        eprintln!("SKIP multicast: send failed");
        let _ = join_or_leave(&recv, group, "0.0.0.0", false);
        return;
    }
    match recv_from_bounded(
        &recv,
        1500,
        Some(Duration::from_millis(400)),
        &CancellationToken::new(),
    )
    .await
    {
        Ok((data, _)) => assert_eq!(data, b"mcast"),
        Err(_) => eprintln!("SKIP multicast: no delivery (env does not route loopback multicast)"),
    }
    let _ = join_or_leave(&recv, group, "0.0.0.0", false);
}

#[test]
fn permission_checks_deny() {
    let c = UdpHandlerConfig {
        allowed_bind_ports: Some(vec![12345]),
        allowed_peers: Some(vec!["10.0.0.".into()]),
        allowed_multicast_groups: Some(vec!["239.1.1.1".into()]),
        max_sockets: Some(2),
    };
    // bind port: 0 (ephemeral) always ok; 12345 ok; 9999 denied.
    assert!(c.check_bind_port(&"0.0.0.0:0".parse().unwrap()).is_ok());
    assert!(c.check_bind_port(&"0.0.0.0:12345".parse().unwrap()).is_ok());
    assert!(c.check_bind_port(&"0.0.0.0:9999".parse().unwrap()).is_err());
    // peer prefix.
    assert!(c.check_peer("10.0.0.5:53").is_ok());
    assert!(c.check_peer("8.8.8.8:53").is_err());
    // multicast group.
    assert!(c.check_group("239.1.1.1").is_ok());
    assert!(c.check_group("239.9.9.9").is_err());
    // default config allows all.
    let open = UdpHandlerConfig::default();
    assert!(open
        .check_bind_port(&"0.0.0.0:9999".parse().unwrap())
        .is_ok());
    assert!(open.check_peer("8.8.8.8:53").is_ok());
}

#[test]
fn interface_and_identity() {
    let h = UdpHandler::new(UdpHandlerConfig::default());
    assert_eq!(h.name(), "udp");
    assert_eq!(h.imports(), Some(vec!["theater:simple/udp".to_string()]));
    assert_eq!(
        h.exports(),
        Some(vec!["theater:simple/udp-client".to_string()])
    );
    // interface hash is deterministic across builds of the same pact.
    let h1 = udp_interface().hash();
    let h2 = udp_interface().hash();
    assert_eq!(h1, h2);
}
