//! FlickerTalk wire protocol (Plan §14, §22–24, §106): types and CBOR encoding, no I/O.
//!
//! - `Packet`: what two peers say to each other (message, receipts, contact card…). Always
//!   encrypted with Olm before it leaves the device, over the DataChannel or through the mailbox.
//! - `Sealed`: the encrypted packet as it travels, with the sender so the receiver can pick the
//!   Olm session.
//! - `Signal`: WebRTC signalling for one peer, relayed by the router; its payload is sealed too.
//!
//! Everything is versioned and backwards compatible (§23): unknown packet types decode as
//! `Body::Unknown` and are ignored, never an error.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

/// Bytes of a file per chunk (§63): one chunk, sealed, fits a single DataChannel message.
pub const FILE_CHUNK: u32 = 48 * 1024;

/// UUIDv7 generated on the device (§24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(Uuid);

impl MessageId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self(Uuid::parse_str(text).context("invalid message id")?))
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Packet {
    pub version: u16,
    pub id: MessageId,
    /// Local clock of the sender, milliseconds since the Unix epoch.
    pub sent_at: u64,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Body {
    Message { text: String },
    /// Receipts (§38): the receiver stored these messages.
    Delivered { ids: Vec<MessageId> },
    Read { ids: Vec<MessageId> },
    /// The receiver has these messages but tells nothing more (issue app#5, app#6): the sender
    /// stops retrying and leaves them as sent, never as delivered (§84). A version that does not
    /// know it ignores it and keeps retrying, which is still correct.
    Received { ids: Vec<MessageId> },
    Typing,
    Ping,
    Pong,
    /// The sender's signed Contact Card (§32), in its own encoding (ft-contacts).
    ContactCard {
        #[serde(with = "serde_bytes")]
        card: Vec<u8>,
        /// The hash of the recipient's route capability the sender uses (app#9): it tells the
        /// recipient which of its hidden sessions, if any, the sender belongs to. Absent from
        /// versions before, which means the main list.
        #[serde(default, with = "serde_bytes")]
        via: Option<Vec<u8>>,
    },
    /// Whether the sender uses the mailbox (§19); travels only between the two devices.
    MailboxPreference { enabled: bool },
    Block,
    /// WebRTC offer, the plaintext of a `Signal`. A first contact adds its Contact Card, so the
    /// recipient can read it and connect without the mailbox.
    Offer {
        sdp: String,
        #[serde(default, with = "serde_bytes")]
        card: Option<Vec<u8>>,
        /// As in `ContactCard`: which of the recipient's capabilities the sender uses.
        #[serde(default, with = "serde_bytes")]
        via: Option<Vec<u8>>,
    },
    /// WebRTC answer, the plaintext of a `Signal`.
    Answer { sdp: String },
    /// A file on offer (§62–63); the packet id identifies it. Only ever sent directly: files never
    /// go through the mailbox. The receiver pulls the chunks with `FileRequest`.
    File {
        name: String,
        size: u64,
        mime: String,
        /// BLAKE3 of the whole file, checked once every chunk is in.
        #[serde(with = "serde_bytes")]
        hash: [u8; 32],
        /// Bytes per chunk; the last one may be shorter.
        chunk: u32,
    },
    /// The receiver asks for `count` chunks from `from` on: flow control, and resuming where it
    /// stopped.
    FileRequest { file: MessageId, from: u64, count: u32 },
    FileChunk {
        file: MessageId,
        index: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The receiver has the whole file and its hash matches.
    FileDone { file: MessageId },
    /// The sender no longer has the file's bytes (it was deleted before the transfer ended): the
    /// receiver marks it failed and stops asking. A version that does not know it keeps asking,
    /// which is what happened before.
    FileFailed { file: MessageId },
    /// A voice or video call (§66). The media is the WebView's WebRTC; its descriptions travel
    /// here, directly and encrypted, never through the mailbox.
    CallOffer { call: MessageId, sdp: String, video: bool },
    CallAnswer { call: MessageId, sdp: String },
    CallEnd { call: MessageId, reason: EndReason },
    /// Moving to a new phone (§60), from the old phone to the new one, only directly. `proof`
    /// shows it read the new phone's QR; `key` seals the database copy that follows, of `size`
    /// bytes and BLAKE3 `hash`, pulled with `MoveRequest` like a file.
    MoveOffer {
        #[serde(with = "serde_bytes")]
        proof: [u8; 32],
        #[serde(with = "serde_bytes")]
        key: [u8; 32],
        size: u64,
        #[serde(with = "serde_bytes")]
        hash: [u8; 32],
    },
    MoveRequest { from: u64, count: u32 },
    MoveChunk {
        index: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// The new phone has the whole copy and its hash matches: the old one can let go.
    MoveDone,
    /// A packet type from a newer version (or one this version cannot read): ignored (§23).
    /// Only ever decoded, never sent.
    #[serde(skip)]
    Unknown,
}

/// Why a call ended, as told to the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// Hung up after it was answered.
    Hangup,
    /// The callee said no.
    Declined,
    /// The callee is in another call.
    Busy,
    /// The caller gave up before it was answered.
    Cancelled,
    /// The media could not connect.
    Failed,
}

impl Packet {
    pub fn new(body: Body) -> Self {
        let sent_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Self { version: PROTOCOL_VERSION, id: MessageId::new(), sent_at, body }
    }

    /// The same message again, for a retry.
    pub fn resend(id: MessageId, sent_at: u64, body: Body) -> Self {
        Self { version: PROTOCOL_VERSION, id, sent_at, body }
    }

    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        // The body is read in two steps so that an unknown type is not an error.
        #[derive(Deserialize)]
        struct Raw {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: ciborium::Value,
        }
        let raw: Raw = decode(bytes)?;
        let body = raw.body.deserialized().unwrap_or(Body::Unknown);
        Ok(Self { version: raw.version, id: raw.id, sent_at: raw.sent_at, body })
    }
}

/// Olm message types: the first messages of a session carry the key exchange (pre-key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealedKind {
    PreKey,
    Normal,
}

/// An encrypted `Packet` as it travels (DataChannel or mailbox).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub version: u16,
    /// Sender's device id, so the receiver can pick the Olm session.
    pub from: String,
    pub kind: SealedKind,
    #[serde(with = "serde_bytes")]
    pub ciphertext: Vec<u8>,
}

impl Sealed {
    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        decode(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Wake,
    Offer,
    Answer,
    Cancel,
}

/// WebRTC signalling for one peer, relayed by the router (§13–14). `sealed` is an encoded
/// `Sealed` whose plaintext is the session description: the router only moves bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub version: u16,
    pub kind: SignalKind,
    /// Random id that ties the offer, the answer and the cancel of one connection attempt.
    pub session: String,
    pub from: String,
    pub to: String,
    #[serde(with = "serde_bytes")]
    pub sealed: Vec<u8>,
}

impl Signal {
    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        decode(bytes)
    }
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).expect("encoding into memory cannot fail");
    bytes
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    ciborium::from_reader(bytes).context("malformed CBOR")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(body: Body) {
        let packet = Packet::new(body);
        let bytes = packet.encode();
        assert_eq!(Packet::decode(&bytes).expect("decodes"), packet);
    }

    #[test]
    fn every_packet_type_survives_the_wire() {
        round_trip(Body::Message { text: "hello".to_owned() });
        round_trip(Body::Delivered { ids: vec![MessageId::new(), MessageId::new()] });
        round_trip(Body::Received { ids: vec![MessageId::new()] });
        round_trip(Body::Read { ids: vec![MessageId::new()] });
        round_trip(Body::Typing);
        round_trip(Body::Ping);
        round_trip(Body::Pong);
        round_trip(Body::ContactCard { card: vec![1, 2, 3], via: None });
        round_trip(Body::ContactCard { card: vec![1, 2, 3], via: Some(vec![9; 32]) });
        round_trip(Body::MailboxPreference { enabled: false });
        round_trip(Body::Block);
        round_trip(Body::Offer { sdp: "v=0".to_owned(), card: Some(vec![1]), via: Some(vec![9; 32]) });
        round_trip(Body::Offer { sdp: "v=0".to_owned(), card: None, via: None });
        round_trip(Body::Answer { sdp: "v=0".to_owned() });
        round_trip(Body::File {
            name: "photo.jpg".to_owned(),
            size: 123_456,
            mime: "image/jpeg".to_owned(),
            hash: [7; 32],
            chunk: FILE_CHUNK,
        });
        let file = MessageId::new();
        round_trip(Body::FileRequest { file, from: 3, count: 16 });
        round_trip(Body::FileChunk { file, index: 3, data: vec![1, 2, 3] });
        round_trip(Body::FileDone { file });
        round_trip(Body::FileFailed { file });
        let call = MessageId::new();
        round_trip(Body::CallOffer { call, sdp: "v=0".to_owned(), video: true });
        round_trip(Body::CallAnswer { call, sdp: "v=0".to_owned() });
        for reason in [EndReason::Hangup, EndReason::Declined, EndReason::Busy, EndReason::Cancelled, EndReason::Failed] {
            round_trip(Body::CallEnd { call, reason });
        }
        round_trip(Body::MoveOffer { proof: [1; 32], key: [2; 32], size: 1 << 20, hash: [3; 32] });
        round_trip(Body::MoveRequest { from: 4, count: 16 });
        round_trip(Body::MoveChunk { index: 4, data: vec![5; 10] });
        round_trip(Body::MoveDone);
    }

    // §62–63: a whole chunk, once sealed with Olm (under 200 bytes more), fits a single
    // DataChannel message (64 KiB, the smallest limit peers announce).
    #[test]
    fn a_file_chunk_fits_one_data_channel_message() {
        let packet = Packet::new(Body::FileChunk { file: MessageId::new(), index: 1 << 40, data: vec![0xab; FILE_CHUNK as usize] });
        assert!(packet.encode().len() + 200 < 64 * 1024, "{} bytes", packet.encode().len());
    }

    // §24: generated on the device, time-ordered, never by a server.
    #[test]
    fn message_ids_are_uuid_v7() {
        let id = MessageId::new();
        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(MessageId::parse(&id.to_string()).expect("parses"), id);
    }

    // §27: a retry resends the same message, so the receiver can drop the duplicate.
    #[test]
    fn a_retried_packet_keeps_its_id_and_time() {
        let original = Packet::new(Body::Message { text: "hi".to_owned() });
        let retry = Packet::resend(original.id, original.sent_at, original.body.clone());
        assert_eq!(retry, original);
    }

    #[test]
    fn packets_carry_the_protocol_version() {
        assert_eq!(Packet::new(Body::Ping).version, PROTOCOL_VERSION);
    }

    // §23: a newer peer may send packet types this version does not know.
    #[test]
    fn unknown_packet_types_are_ignored_not_rejected() {
        #[derive(serde::Serialize)]
        struct Future {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: FutureBody,
        }
        #[derive(serde::Serialize)]
        #[serde(tag = "type", content = "data", rename_all = "snake_case")]
        enum FutureBody {
            Hologram { depth: u8 },
        }
        let future = Future { version: 9, id: MessageId::new(), sent_at: 1, body: FutureBody::Hologram { depth: 3 } };
        let mut bytes = Vec::new();
        ciborium::into_writer(&future, &mut bytes).expect("encodes");

        let decoded = Packet::decode(&bytes).expect("still decodes");
        assert_eq!(decoded.body, Body::Unknown);
        assert_eq!(decoded.version, 9);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(Packet::decode(&[0xff, 0x00, 0x13]).is_err());
    }

    #[test]
    fn sealed_packets_survive_the_wire() {
        let sealed = Sealed {
            version: PROTOCOL_VERSION,
            from: "ft_sender".to_owned(),
            kind: SealedKind::PreKey,
            ciphertext: vec![9; 40],
        };
        assert_eq!(Sealed::decode(&sealed.encode()).expect("decodes"), sealed);
    }

    #[test]
    fn signals_survive_the_wire() {
        let signal = Signal {
            version: PROTOCOL_VERSION,
            kind: SignalKind::Offer,
            session: "s-1".to_owned(),
            from: "ft_caller".to_owned(),
            to: "ft_callee".to_owned(),
            sealed: vec![4; 12],
        };
        assert_eq!(Signal::decode(&signal.encode()).expect("decodes"), signal);
    }

    // CBOR keeps signalling small for push (§15): bytes stay bytes, not base64 text.
    #[test]
    fn ciphertext_is_encoded_as_raw_bytes() {
        let sealed = Sealed { version: 1, from: String::new(), kind: SealedKind::Normal, ciphertext: vec![0; 1000] };
        assert!(sealed.encode().len() < 1100);
    }
}
