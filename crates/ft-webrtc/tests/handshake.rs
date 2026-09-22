//! PoC 0 (Plan §87, §103): two peers connect and exchange "hello" over a data channel.
//! Signalling here is a plain in-process pipe; on a phone it travels encrypted through push (§13).

use std::time::Duration;

use ft_webrtc::{Role, Session, SessionConfig, Signal};
use tokio::sync::mpsc;
use tokio::time::timeout;

const LIMIT: Duration = Duration::from_secs(20);

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
    let received = timeout(LIMIT, callee_inbox.next()).await.expect("the callee receives in time");
    assert_eq!(received.as_deref(), Some("hello"));

    callee.send("hello back").await.expect("the callee sends");
    let received = timeout(LIMIT, caller_inbox.next()).await.expect("the caller receives in time");
    assert_eq!(received.as_deref(), Some("hello back"));

    caller.close().await.expect("the caller closes");
    callee.close().await.expect("the callee closes");
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
    let received = timeout(LIMIT, callee_inbox.next()).await.expect("received in time");
    assert_eq!(received.as_deref(), Some("hello"));
}
