//! Two sessions connect through a signalling relay, as the phone and the Mac will in PoC 0 B1.

use std::sync::Arc;
use std::time::Duration;

use ft_webrtc::{Role, SessionConfig};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

const LIMIT: Duration = Duration::from_secs(20);

/// A two-peer relay with the same behaviour as server/ft-router's PoC relay: a welcome with the
/// ICE servers first (none here: the test stays offline). `None` mimics an older relay without it.
async fn start_relay(welcome: Option<&'static str>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let address = listener.local_addr().expect("has an address");
    let peers: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>> = Arc::default();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(stream, peers.clone(), welcome));
        }
    });
    format!("ws://{address}")
}

async fn serve(
    stream: TcpStream,
    peers: Arc<Mutex<Vec<mpsc::UnboundedSender<String>>>>,
    welcome: Option<&'static str>,
) {
    let socket = tokio_tungstenite::accept_async(stream).await.expect("upgrades");
    let (mut sink, mut source) = socket.split();
    let (outbox, mut pending) = mpsc::unbounded_channel::<String>();
    if let Some(welcome) = welcome {
        let _ = outbox.send(welcome.to_owned());
    }
    let index = {
        let mut all = peers.lock().await;
        if let Some(first) = all.first() {
            let _ = first.send(r#"{"kind":"peer_joined"}"#.to_owned());
            let _ = outbox.send(r#"{"kind":"peer_present"}"#.to_owned());
        }
        all.push(outbox);
        all.len() - 1
    };
    tokio::spawn(async move {
        while let Some(text) = pending.recv().await {
            let _ = sink.send(Message::Text(text.into())).await;
        }
    });
    while let Some(Ok(Message::Text(text))) = source.next().await {
        for (other, outbox) in peers.lock().await.iter().enumerate() {
            if other != index {
                let _ = outbox.send(text.to_string());
            }
        }
    }
}

const WELCOME: &str = r#"{"kind":"welcome","stun":[],"turn":null}"#;

#[tokio::test(flavor = "multi_thread")]
async fn two_sessions_connect_through_the_relay_and_talk() {
    talk_through(&start_relay(Some(WELCOME)).await).await;
}

// The first message is not lost when it is not a welcome.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_relay_without_a_welcome_still_works() {
    talk_through(&start_relay(None).await).await;
}

async fn talk_through(relay: &str) {
    let (callee, mut callee_inbox) = ft_poc::connect(relay, "demo", Role::Callee, SessionConfig::offline())
        .await
        .expect("the callee joins");
    let (caller, mut caller_inbox) = ft_poc::connect(relay, "demo", Role::Caller, SessionConfig::offline())
        .await
        .expect("the caller joins");

    timeout(LIMIT, caller.wait_open()).await.expect("opens in time").expect("opens");
    caller.send("hello").await.expect("the caller sends");
    assert_eq!(timeout(LIMIT, callee_inbox.next_text()).await.expect("in time").as_deref(), Some("hello"));

    callee.send("hello back").await.expect("the callee answers");
    assert_eq!(timeout(LIMIT, caller_inbox.next_text()).await.expect("in time").as_deref(), Some("hello back"));
}
