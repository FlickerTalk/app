//! PoC 0 (Plan §87, §103): two peers connect and exchange "hello" over a data channel.
//! Signalling here is a plain in-process pipe; on a phone it travels encrypted through push (§13).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ft_webrtc::{Inbox, Role, Session, SessionConfig, Signal, TurnServer};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(20);

/// A STUN and a TURN server that never answer: local UDP sockets nobody reads (a server behind a
/// network that drops the packets, or an interface with no way out). Nothing leaves the machine.
struct Blackhole {
    _stun: std::net::UdpSocket,
    _turn: std::net::UdpSocket,
    config: SessionConfig,
}

fn blackhole() -> Blackhole {
    let stun = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
    let turn = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
    let config = SessionConfig {
        stun_servers: vec![format!("stun:{}", stun.local_addr().expect("its address"))],
        turn_servers: vec![TurnServer {
            url: format!("turn:{}?transport=udp", turn.local_addr().expect("its address")),
            username: "u".to_owned(),
            credential: "c".to_owned(),
        }],
        ..SessionConfig::offline()
    };
    Blackhole { _stun: stun, _turn: turn, config }
}

fn pipe(signals: mpsc::Receiver<Signal>, peer: Session) {
    let mut signals = signals;
    tokio::spawn(async move {
        while let Some(signal) = signals.recv().await {
            peer.handle_signal(signal).await.expect("the peer handles the signal");
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn two_peers_exchange_messages_over_the_data_channel() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);

    let (caller, mut caller_inbox) = Session::start(SessionConfig::offline(), Role::Caller, caller_out)
        .await
        .expect("the caller starts");
    let (callee, mut callee_inbox) = Session::start(SessionConfig::offline(), Role::Callee, callee_out)
        .await
        .expect("the callee starts");

    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());

    caller.invite().await.expect("the caller sends the offer");

    timeout(LIMIT, caller.wait_open())
        .await
        .expect("the data channel opens before the timeout")
        .expect("the data channel opens");

    caller.send("hello").await.expect("the caller sends");
    let received = timeout(LIMIT, callee_inbox.next_text()).await.expect("the callee receives in time");
    assert_eq!(received.as_deref(), Some("hello"));

    callee.send("hello back").await.expect("the callee sends");
    let received = timeout(LIMIT, caller_inbox.next_text()).await.expect("the caller receives in time");
    assert_eq!(received.as_deref(), Some("hello back"));

    caller.close().await.expect("the caller closes");
    callee.close().await.expect("the callee closes");
}

/// Encrypted packets are binary: they must arrive byte for byte.
#[tokio::test(flavor = "multi_thread")]
async fn binary_packets_cross_the_data_channel_unchanged() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(SessionConfig::offline(), Role::Caller, caller_out).await.expect("starts");
    let (callee, mut callee_inbox) = Session::start(SessionConfig::offline(), Role::Callee, callee_out).await.expect("starts");
    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());
    caller.invite().await.expect("offers");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");

    let packet = vec![0u8, 255, 1, 2, 0xc3, 0x28, 7];
    caller.send_bytes(&packet).await.expect("sends");
    assert_eq!(timeout(LIMIT, callee_inbox.next()).await.expect("in time"), Some(packet));
}

/// Closing a session ends the other side's channel too: its inbox finishes and it stops being open.
#[tokio::test(flavor = "multi_thread")]
async fn closing_one_side_closes_the_other() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(SessionConfig::offline(), Role::Caller, caller_out).await.expect("starts");
    let (callee, mut callee_inbox) = Session::start(SessionConfig::offline(), Role::Callee, callee_out).await.expect("starts");
    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());
    caller.invite().await.expect("offers");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    timeout(LIMIT, callee.wait_open()).await.expect("opens in time").expect("opens");

    caller.close().await.expect("closes");
    assert!(!caller.is_open());
    let ended = timeout(Duration::from_secs(5), callee_inbox.next()).await.expect("the other side sees the close");
    assert_eq!(ended, None);
    assert!(!callee.is_open());
}

/// Same exchange, gathering candidates through Google's STUN over the real network (Plan §16:
/// allowed during the PoC). Ignored by default because it needs internet:
/// `cargo test -p ft-webrtc -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs internet access"]
async fn two_peers_connect_through_stun_on_the_real_network() {
    let stun = || SessionConfig::with_stun(["stun:stun.l.google.com:19302".to_owned()]);
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(stun(), Role::Caller, caller_out).await.expect("the caller starts");
    let (callee, mut callee_inbox) =
        Session::start(stun(), Role::Callee, callee_out).await.expect("the callee starts");

    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());

    caller.invite().await.expect("the caller sends the offer");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    caller.send("hello").await.expect("the caller sends");
    let received = timeout(LIMIT, callee_inbox.next_text()).await.expect("received in time");
    assert_eq!(received.as_deref(), Some("hello"));
}

/// The local coturn of `app/CLAUDE.md` by default. `FT_TURN_URL`, `FT_TURN_USERNAME` and
/// `FT_TURN_CREDENTIAL` point the test at another relay, such as the cluster's.
fn relayed() -> SessionConfig {
    let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
    let url = var("FT_TURN_URL", "turn:127.0.0.1:3478");
    let base = if url.contains("127.0.0.1") { SessionConfig::offline() } else { SessionConfig::default() };
    SessionConfig {
        turn_servers: vec![TurnServer {
            url,
            username: var("FT_TURN_USERNAME", "ft"),
            credential: var("FT_TURN_CREDENTIAL", "poc"),
        }],
        relay_only: true,
        ..base
    }
}

/// Same exchange with "Always relay" (Plan §17): only relay candidates, so every packet goes
/// through the TURN server and neither peer sees the other's IP. Needs a TURN relay (see
/// `relayed`): `cargo test -p ft-webrtc -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a TURN relay"]
async fn two_peers_connect_through_the_turn_relay_alone() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(relayed(), Role::Caller, caller_out).await.expect("the caller starts");
    let (callee, mut callee_inbox) =
        Session::start(relayed(), Role::Callee, callee_out).await.expect("the callee starts");

    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());

    caller.invite().await.expect("the caller sends the offer");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    caller.send("hello").await.expect("the caller sends");
    let received = timeout(LIMIT, callee_inbox.next_text()).await.expect("received in time");
    assert_eq!(received.as_deref(), Some("hello"));
}

/// A peer that disappears without closing (its app killed, the phone reinstalled) must not look
/// open for ever: the channel's close never comes, so the connection's own state has to end it.
/// Otherwise everything sent to it vanishes and nothing falls back to the mailbox.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_vanishes_stops_being_open() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(SessionConfig::offline(), Role::Caller, caller_out).await.expect("starts");
    let (callee, _) = Session::start(SessionConfig::offline(), Role::Callee, callee_out).await.expect("starts");
    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());
    caller.invite().await.expect("offers");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    timeout(LIMIT, callee.wait_open()).await.expect("opens in time").expect("opens");

    callee.vanish().await;
    let mut waited = Duration::ZERO;
    while caller.is_open() && waited < Duration::from_secs(40) {
        tokio::time::sleep(Duration::from_millis(250)).await;
        waited += Duration::from_millis(250);
    }
    assert!(!caller.is_open(), "the caller still thinks a vanished peer is there");
}

/// The same vanished peer must also end the other side's inbox (app#151): that is what tells the
/// core the link is gone for good. Not when the connection is first disconnected (a few seconds
/// without a packet), since it may come back from that (app#152), but once it has failed: about
/// 30 s with webrtc's defaults (5 s disconnected, 25 s more).
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_vanishes_ends_the_other_side_s_inbox() {
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, mut caller_inbox) = Session::start(SessionConfig::offline(), Role::Caller, caller_out).await.expect("starts");
    let (callee, _) = Session::start(SessionConfig::offline(), Role::Callee, callee_out).await.expect("starts");
    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());
    caller.invite().await.expect("offers");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    timeout(LIMIT, callee.wait_open()).await.expect("opens in time").expect("opens");

    let vanished = std::time::Instant::now();
    callee.vanish().await;
    let ended = timeout(Duration::from_secs(45), caller_inbox.next()).await.expect("the inbox ends once the connection has failed");
    eprintln!("a vanished peer ended the inbox after {} ms", vanished.elapsed().as_millis());
    assert_eq!(ended, None);
    assert!(!caller.is_open());
}

// DataChannel setup time (2026-09-29): a STUN or TURN server that never answers held the offer
// and then the answer for the whole 3 s gathering deadline each, so the channel took over 6 s to
// open. Each description now goes once it has what the routing wants, or a second at most.
#[tokio::test(flavor = "multi_thread")]
async fn with_servers_that_never_answer_the_channel_still_opens_quickly() {
    let hole = blackhole();
    let (caller_out, caller_signals) = mpsc::channel(32);
    let (callee_out, callee_signals) = mpsc::channel(32);
    let (caller, _) = Session::start(hole.config.clone(), Role::Caller, caller_out).await.expect("starts");
    let (callee, mut callee_inbox) = Session::start(hole.config.clone(), Role::Callee, callee_out).await.expect("starts");
    pipe(caller_signals, callee.clone());
    pipe(callee_signals, caller.clone());

    let started = std::time::Instant::now();
    caller.invite().await.expect("offers");
    let offered = started.elapsed();
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    let opened = started.elapsed();
    eprintln!("unreachable servers: offer sent after {} ms, channel open after {} ms", offered.as_millis(), opened.as_millis());
    let gathering = caller.gathering();
    assert!(!gathering.complete, "cut short");
    assert_eq!((gathering.host, gathering.srflx, gathering.relay), (1, 0, 0));
    assert!(offered < Duration::from_millis(1_500), "the offer waited {offered:?}");
    assert!(opened < Duration::from_millis(3_000), "the channel opened after {opened:?}");

    caller.send("hello").await.expect("sends");
    assert_eq!(timeout(LIMIT, callee_inbox.next_text()).await.expect("in time").as_deref(), Some("hello"));
    caller.close().await.expect("closes");
    callee.close().await.expect("closes");
}

/// The port of the first host candidate of a description.
fn host_port(sdp: &str) -> u16 {
    sdp.lines()
        .filter(|line| line.starts_with("a=candidate:") && line.contains(" typ host"))
        .find_map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            let typ = words.iter().position(|word| *word == "typ")?;
            words.get(typ.checked_sub(1)?)?.parse().ok()
        })
        .expect("a host candidate")
}

/// The description with its host candidate moved to `port`; and the port it had.
fn moved_to(signal: Signal, port: u16) -> (Signal, u16) {
    let Signal::Sdp(json) = signal else { panic!("a description") };
    let mut description: serde_json::Value = serde_json::from_str(&json).expect("json");
    let sdp = description["sdp"].as_str().expect("an sdp").to_owned();
    let was = host_port(&sdp);
    let sdp = sdp.replace(&format!(" 127.0.0.1 {was} typ host"), &format!(" 127.0.0.1 {port} typ host"));
    description["sdp"] = serde_json::Value::String(sdp);
    (Signal::Sdp(description.to_string()), was)
}

/// Two peers whose every packet goes through a cable that can be pulled out for a while (a phone
/// that loses its network for a few seconds, a Wi-Fi that stalls): a UDP forwarder, each side
/// seeing the other at one of its two ends. Returns the caller, the callee, their inboxes and
/// whether the cable is out.
async fn through_a_cable() -> (Session, Inbox, Session, Inbox, Arc<AtomicBool>) {
    let out = Arc::new(AtomicBool::new(false));
    let caller_end = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("a socket"));
    let callee_end = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("a socket"));
    let caller_at: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
    let callee_at: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
    // What the callee sends to the caller's end goes on to the caller from the callee's end, and
    // the other way round.
    for (from, to, peer) in [(caller_end.clone(), callee_end.clone(), caller_at.clone()), (callee_end.clone(), caller_end.clone(), callee_at.clone())] {
        let out = out.clone();
        tokio::spawn(async move {
            let mut packet = vec![0u8; 65_536];
            while let Ok((size, _)) = from.recv_from(&mut packet).await {
                let peer = *peer.lock().expect("peer");
                if let (Some(peer), false) = (peer, out.load(Ordering::SeqCst)) {
                    let _ = to.send_to(&packet[..size], peer).await;
                }
            }
        });
    }

    let (caller_out, mut caller_signals) = mpsc::channel(32);
    let (callee_out, mut callee_signals) = mpsc::channel(32);
    let (caller, caller_inbox) = Session::start(SessionConfig::offline(), Role::Caller, caller_out).await.expect("starts");
    let (callee, callee_inbox) = Session::start(SessionConfig::offline(), Role::Callee, callee_out).await.expect("starts");
    let (to_callee, end, at) = (callee.clone(), caller_end.local_addr().expect("an address").port(), caller_at);
    tokio::spawn(async move {
        while let Some(signal) = caller_signals.recv().await {
            let (signal, was) = moved_to(signal, end);
            *at.lock().expect("at") = Some(SocketAddr::from(([127, 0, 0, 1], was)));
            to_callee.handle_signal(signal).await.expect("the callee handles it");
        }
    });
    let (to_caller, end, at) = (caller.clone(), callee_end.local_addr().expect("an address").port(), callee_at);
    tokio::spawn(async move {
        while let Some(signal) = callee_signals.recv().await {
            let (signal, was) = moved_to(signal, end);
            *at.lock().expect("at") = Some(SocketAddr::from(([127, 0, 0, 1], was)));
            to_caller.handle_signal(signal).await.expect("the caller handles it");
        }
    });
    caller.invite().await.expect("offers");
    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    timeout(LIMIT, callee.wait_open()).await.expect("opens in time").expect("opens");
    (caller, caller_inbox, callee, callee_inbox, out)
}

async fn until_open(session: &Session, open: bool, limit: Duration) -> Duration {
    let started = std::time::Instant::now();
    while session.is_open() != open {
        assert!(started.elapsed() < limit, "still {} after {limit:?}", if open { "closed" } else { "open" });
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    started.elapsed()
}

/// A few seconds without a packet (the network stalls, then comes back): the connection says it
/// is disconnected and then connected again, and the data channel works all along. It counts as
/// open again, and its inbox goes on (app#152: it stayed "not open" for good, so the chat said
/// "Not connected" until a send dropped it and opened another).
#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_comes_back_after_a_blip_is_open_again() {
    let (caller, mut caller_inbox, callee, mut callee_inbox, out) = through_a_cable().await;
    caller.send("before").await.expect("sends");
    assert_eq!(timeout(LIMIT, callee_inbox.next_text()).await.expect("in time").as_deref(), Some("before"));

    out.store(true, Ordering::SeqCst);
    let lost = until_open(&caller, false, Duration::from_secs(20)).await;
    until_open(&callee, false, Duration::from_secs(20)).await;
    out.store(false, Ordering::SeqCst);
    let back = until_open(&caller, true, Duration::from_secs(15)).await;
    until_open(&callee, true, Duration::from_secs(15)).await;
    eprintln!("a blip: not open after {} ms without packets, open again {} ms after they flow", lost.as_millis(), back.as_millis());

    caller.send("after").await.expect("sends");
    assert_eq!(timeout(LIMIT, callee_inbox.next_text()).await.expect("in time").as_deref(), Some("after"));
    callee.send("both ways").await.expect("sends");
    assert_eq!(timeout(LIMIT, caller_inbox.next_text()).await.expect("in time").as_deref(), Some("both ways"));
}
