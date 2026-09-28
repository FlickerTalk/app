//! A voice call's peer connection: one audio track, Opus only, descriptions sent whole with their
//! ICE candidates (no trickle), as the WebView's calls and the core's data channels do.

use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{anyhow, bail, Context, Result};
use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;
use tokio::sync::{oneshot, watch};
use webrtc::media_stream::track_remote::TrackRemote;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceGatheringState, RTCPeerConnectionState,
    RTCSessionDescription,
};
use webrtc_engine::rtp::{add_audio_track, peer_connection_builder, AudioSender};

use crate::{ice_setup, MediaConfig};

/// How far the connection has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    /// It could not connect, or it stopped answering for good.
    Failed,
    Closed,
}

type PendingTrack = Mutex<Option<oneshot::Sender<Arc<dyn TrackRemote>>>>;

struct Events {
    gathered: watch::Sender<bool>,
    state: Arc<watch::Sender<LinkState>>,
    track: PendingTrack,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Events {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered.send(true);
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let state = match state {
            RTCPeerConnectionState::Connected => LinkState::Connected,
            RTCPeerConnectionState::Failed => LinkState::Failed,
            RTCPeerConnectionState::Closed => LinkState::Closed,
            // Disconnected may come back on its own: ICE keeps trying.
            _ => return,
        };
        let _ = self.state.send(state);
    }

    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        if track.kind().await != RtpCodecKind::Audio {
            return;
        }
        let pending = self.track.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(pending) = pending {
            let _ = pending.send(track);
        }
    }
}

/// One side of a voice call's connection.
pub struct MediaSession {
    connection: Arc<dyn PeerConnection>,
    sender: Arc<AudioSender>,
    gathered: watch::Receiver<bool>,
    state: watch::Receiver<LinkState>,
    /// Closing says so itself: webrtc-rs does not report its own close.
    closed: Arc<watch::Sender<LinkState>>,
    remote: Mutex<Option<oneshot::Receiver<Arc<dyn TrackRemote>>>>,
    config: MediaConfig,
}

impl MediaSession {
    /// A connection with our audio track, ready to offer or to answer.
    pub async fn open(config: &MediaConfig) -> Result<Self> {
        let (servers, policy) = ice_setup(config);
        let (gathered_tx, gathered) = watch::channel(false);
        let (state_tx, state) = watch::channel(LinkState::Connecting);
        let state_tx = Arc::new(state_tx);
        let (track_tx, track) = oneshot::channel();
        let events = Events { gathered: gathered_tx, state: state_tx.clone(), track: Mutex::new(Some(track_tx)) };
        let connection: Arc<dyn PeerConnection> = Arc::new(
            peer_connection_builder()?
                .with_configuration(
                    RTCConfigurationBuilder::new().with_ice_servers(servers).with_ice_transport_policy(policy).build(),
                )
                .with_handler(Arc::new(events))
                .with_udp_addrs(config.bind.clone())
                .build()
                .await?,
        );
        let sender = match add_audio_track(connection.as_ref()).await {
            Ok(sender) => sender,
            Err(error) => {
                let _ = connection.close().await;
                return Err(error.into());
            }
        };
        Ok(Self {
            connection,
            sender: Arc::new(sender),
            gathered,
            state,
            closed: state_tx,
            remote: Mutex::new(Some(track)),
            config: config.clone(),
        })
    }

    /// Our offer, once ICE gathering is over (or its deadline passed).
    pub async fn offer(&self) -> Result<String> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;
        self.gathered_description().await
    }

    /// Our answer to `offer`, once gathered. Only the audio is answered: a video line is refused,
    /// so a WebView's video offer gets a voice call.
    pub async fn answer(&self, offer: &str) -> Result<String> {
        let offer = RTCSessionDescription::offer(offer.to_owned()).context("invalid offer")?;
        self.connection.set_remote_description(offer).await?;
        let answer = self.connection.create_answer(None).await?;
        self.connection.set_local_description(answer).await?;
        self.gathered_description().await
    }

    /// The other side's answer to our offer.
    pub async fn accept(&self, answer: &str) -> Result<()> {
        let answer = RTCSessionDescription::answer(answer.to_owned()).context("invalid answer")?;
        self.connection.set_remote_description(answer).await?;
        Ok(())
    }

    /// The local description with the candidates gathered so far: once gathering completes or,
    /// at most, when the deadline passes. An interface that never answers must not hold the call.
    async fn gathered_description(&self) -> Result<String> {
        let mut gathered = self.gathered.clone();
        let _ = tokio::time::timeout(self.config.gather_timeout, gathered.wait_for(|done| *done)).await;
        let description = self.connection.local_description().await.ok_or_else(|| anyhow!("no local description"))?;
        if !description.sdp.contains("a=candidate:") {
            bail!("no ICE candidate was gathered");
        }
        Ok(description.sdp)
    }

    /// Follows the connection.
    pub fn state(&self) -> watch::Receiver<LinkState> {
        self.state.clone()
    }

    /// Our voice goes out here.
    pub fn sender(&self) -> Arc<AudioSender> {
        self.sender.clone()
    }

    /// The other side's audio track, handed over when its first packet arrives. Only once.
    pub fn take_remote(&self) -> Option<oneshot::Receiver<Arc<dyn TrackRemote>>> {
        self.remote.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    pub async fn close(&self) {
        let _ = self.connection.close().await;
        let _ = self.closed.send(LinkState::Closed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const LIMIT: Duration = Duration::from_secs(10);

    async fn wait_until(session: &MediaSession, wanted: LinkState) {
        let mut state = session.state();
        tokio::time::timeout(LIMIT, state.wait_for(|state| *state == wanted))
            .await
            .unwrap_or_else(|_| panic!("never {wanted:?}"))
            .expect("the session is there");
    }

    fn sections(sdp: &str) -> Vec<String> {
        sdp.split("\r\nm=").skip(1).map(|section| format!("m={section}")).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_sessions_connect_with_one_offer_and_one_answer() {
        let caller = MediaSession::open(&MediaConfig::default()).await.expect("caller");
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");

        let offer = caller.offer().await.expect("offer");
        assert!(offer.contains("m=audio "), "{offer}");
        assert!(offer.contains("a=rtpmap:111 opus/48000/2"), "{offer}");
        assert!(offer.contains("a=candidate:"), "the candidates travel in the offer: {offer}");

        let answer = callee.answer(&offer).await.expect("answer");
        assert!(answer.contains("a=candidate:"), "{answer}");
        caller.accept(&answer).await.expect("accepted");

        wait_until(&caller, LinkState::Connected).await;
        wait_until(&callee, LinkState::Connected).await;

        caller.close().await;
        wait_until(&caller, LinkState::Closed).await;
        callee.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_description_that_is_not_sdp_is_refused() {
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        assert!(callee.answer("hello").await.is_err());
        callee.close().await;
    }

    // What a WebView (Chrome) offers for a video call, as an older app would send it.
    fn webview_video_offer() -> String {
        let fingerprint = (0..32).map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(":");
        let transport = |mid: &str| {
            vec![
                "c=IN IP4 0.0.0.0".to_owned(),
                "a=rtcp:9 IN IP4 0.0.0.0".to_owned(),
                "a=candidate:1 1 udp 2122260223 127.0.0.1 50000 typ host generation 0".to_owned(),
                "a=ice-ufrag:WbVw".to_owned(),
                "a=ice-pwd:webviewwebviewwebview123".to_owned(),
                format!("a=fingerprint:sha-256 {fingerprint}"),
                "a=setup:actpass".to_owned(),
                format!("a=mid:{mid}"),
                "a=sendrecv".to_owned(),
                "a=rtcp-mux".to_owned(),
            ]
        };
        let mut lines = vec![
            "v=0".to_owned(),
            "o=- 4611731400430051336 2 IN IP4 127.0.0.1".to_owned(),
            "s=-".to_owned(),
            "t=0 0".to_owned(),
            "a=group:BUNDLE 0 1".to_owned(),
            "a=msid-semantic: WMS stream".to_owned(),
            "m=audio 9 UDP/TLS/RTP/SAVPF 111 9 0 8 126".to_owned(),
        ];
        lines.extend(transport("0"));
        lines.extend(
            [
                "a=msid:stream voice",
                "a=rtpmap:111 opus/48000/2",
                "a=fmtp:111 minptime=10;useinbandfec=1",
                "a=rtpmap:9 G722/8000",
                "a=rtpmap:0 PCMU/8000",
                "a=rtpmap:8 PCMA/8000",
                "a=rtpmap:126 telephone-event/8000",
                "a=ssrc:1111 cname:webview",
                "m=video 9 UDP/TLS/RTP/SAVPF 96 97",
            ]
            .map(str::to_owned),
        );
        lines.extend(transport("1"));
        lines.extend(
            [
                "a=msid:stream camera",
                "a=rtpmap:96 VP8/90000",
                "a=rtcp-fb:96 nack",
                "a=rtcp-fb:96 nack pli",
                "a=rtpmap:97 rtx/90000",
                "a=fmtp:97 apt=96",
                "a=ssrc-group:FID 2222 3333",
                "a=ssrc:2222 cname:webview",
                "a=ssrc:3333 cname:webview",
            ]
            .map(str::to_owned),
        );
        lines.join("\r\n") + "\r\n"
    }

    // Interop: an older app's WebView may offer video; the native side answers the voice only.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_webview_offer_with_video_is_answered_with_audio_only() {
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        let answer = callee.answer(&webview_video_offer()).await.expect("answer");
        let sections = sections(&answer);

        let audio = sections.iter().find(|section| section.starts_with("m=audio")).expect("an audio line");
        assert!(!audio.starts_with("m=audio 0 "), "{answer}");
        assert!(audio.contains("a=rtpmap:111 opus/48000/2"), "{answer}");
        assert!(audio.contains("a=sendrecv"), "{answer}");

        // A refused line (port 0) or an inactive one: either way no video flows.
        for video in sections.iter().filter(|section| section.starts_with("m=video")) {
            assert!(video.starts_with("m=video 0 ") || video.contains("a=inactive"), "video is not refused:\n{answer}");
        }
        callee.close().await;
    }
}
