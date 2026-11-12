//! Unknown packet types must decode as Body::Unknown and be ignored, or older apps break.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

// 1 = audio and video lines in every call plus CallMedia, 2 = also CallPresent. Only send what the
// other side said it speaks.
pub const CALL_MEDIA_VERSION: u16 = 2;

pub const CALL_MEDIA_CAMERA: u16 = 1;

pub const CALL_MEDIA_PRESENT: u16 = 2;

// padding hides message sizes from the router, same bucket as Signal
pub const PAD_BUCKET: usize = 160;

// a sealed chunk has to fit in one DataChannel message
pub const FILE_CHUNK: u32 = 48 * 1024;

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
    // ms, sender's clock
    pub sent_at: u64,
    pub body: Body,
    // None from older apps, which isn't "off"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Body {
    Message {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<MessageId>,
    },
    Delivered { ids: Vec<MessageId> },
    Read { ids: Vec<MessageId> },
    // the sender stops retrying but must not show these as delivered
    Received { ids: Vec<MessageId> },
    Typing,
    Ping,
    Pong,
    ContactCard {
        #[serde(with = "serde_bytes")]
        card: Vec<u8>,
        // absent = main list
        #[serde(default, with = "serde_bytes")]
        via: Option<Vec<u8>>,
    },
    MailboxPreference { enabled: bool },
    Block,
    Offer {
        sdp: String,
        #[serde(default, with = "serde_bytes")]
        card: Option<Vec<u8>>,
        #[serde(default, with = "serde_bytes")]
        via: Option<Vec<u8>>,
    },
    Answer { sdp: String },
    // files never go through the mailbox
    File {
        name: String,
        size: u64,
        mime: String,
        #[serde(with = "serde_bytes")]
        hash: [u8; 32],
        chunk: u32,
    },
    FileRequest { file: MessageId, from: u64, count: u32 },
    FileChunk {
        file: MessageId,
        index: u64,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    FileDone { file: MessageId },
    FileFailed { file: MessageId },
    // never through the mailbox
    CallOffer {
        call: MessageId,
        sdp: String,
        video: bool,
        #[serde(default)]
        media: u16,
    },
    CallAnswer {
        call: MessageId,
        sdp: String,
        #[serde(default)]
        media: u16,
    },
    CallEnd { call: MessageId, reason: EndReason },
    // carries the whole camera state, the highest seq wins
    CallMedia { call: MessageId, seq: u32, video: bool, paused: bool },
    CallPresent { call: MessageId, seq: u32, plugin: Option<String>, file: Option<MessageId> },
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
    MoveDone,
    CircleCard {
        #[serde(with = "serde_bytes")]
        card: Vec<u8>,
    },
    CircleMessage { circle: String, text: String },
    CircleLeave { circle: String },
    // only over the direct connection, never the mailbox
    PluginEvent {
        plugin: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    Reaction { to: MessageId, emoji: String },
    Edit { of: MessageId, text: String },
    Delete { of: MessageId },
    // only decoded, never sent
    #[serde(skip)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Hangup,
    Declined,
    Busy,
    Cancelled,
    Failed,
}

impl Packet {
    pub fn new(body: Body) -> Self {
        let sent_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Self { version: PROTOCOL_VERSION, id: MessageId::new(), sent_at, body, mailbox: None }
    }

    pub fn resend(id: MessageId, sent_at: u64, body: Body) -> Self {
        Self { version: PROTOCOL_VERSION, id, sent_at, body, mailbox: None }
    }

    pub fn encode(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Padded<'a> {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: &'a Body,
            #[serde(skip_serializing_if = "Option::is_none")]
            mailbox: Option<bool>,
            #[serde(with = "serde_bytes")]
            pad: Vec<u8>,
        }
        let with_pad = |pad: usize| {
            encode(&Padded { version: self.version, id: self.id, sent_at: self.sent_at, body: &self.body, mailbox: self.mailbox, pad: vec![0; pad] })
        };
        let bare = with_pad(0);
        // the pad's length prefix grows with the pad, so loop until the size lands exactly
        let mut missing = (PAD_BUCKET - bare.len() % PAD_BUCKET) % PAD_BUCKET;
        loop {
            let padded = with_pad(missing);
            if padded.len().is_multiple_of(PAD_BUCKET) {
                return padded;
            }
            missing += PAD_BUCKET - padded.len() % PAD_BUCKET;
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        // two steps so an unknown type isn't an error, the pad is dropped as an unknown field
        #[derive(Deserialize)]
        struct Raw {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: ciborium::Value,
            #[serde(default)]
            mailbox: Option<bool>,
        }
        let raw: Raw = decode(bytes)?;
        let body = raw.body.deserialized().unwrap_or(Body::Unknown);
        Ok(Self { version: raw.version, id: raw.id, sent_at: raw.sent_at, body, mailbox: raw.mailbox })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SealedKind {
    PreKey,
    Normal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub version: u16,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub version: u16,
    pub kind: SignalKind,
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

// sealed again for the recipient so the router sees neither sender nor pre-key identity key
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    // 1 = NaCl sealed box
    pub envelope: u16,
    #[serde(with = "serde_bytes")]
    pub sealed: Vec<u8>,
}

pub const ENVELOPE_VERSION: u16 = 1;

impl Envelope {
    pub fn encode(&self) -> Vec<u8> {
        encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        decode(bytes)
    }

    pub fn is_envelope(bytes: &[u8]) -> bool {
        Self::decode(bytes).is_ok()
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
    fn a_text_without_reply_to_is_a_plain_message() {
        let plain = Packet::new(Body::Message { text: "hi".to_owned(), reply_to: None });
        let bytes = plain.encode();
        assert!(!String::from_utf8_lossy(&bytes).contains("reply_to"), "nothing on the wire for a plain text");
        assert_eq!(Packet::decode(&bytes).unwrap().body, Body::Message { text: "hi".to_owned(), reply_to: None });
    }

    #[test]
    fn a_packet_says_whether_its_sender_uses_the_mailbox() {
        for mailbox in [Some(true), Some(false)] {
            let packet = Packet { mailbox, ..Packet::new(Body::Message { text: "hi".to_owned(), reply_to: None }) };
            assert_eq!(Packet::decode(&packet.encode()).expect("decodes").mailbox, mailbox);
        }
        let silent = Packet::new(Body::Ping);
        assert_eq!(silent.mailbox, None);
        assert!(!String::from_utf8_lossy(&silent.encode()).contains("mailbox"), "nothing on the wire");
    }

    fn packet_before_the_mailbox_stamp(body: &Body) -> Vec<u8> {
        #[derive(serde::Serialize)]
        struct Before<'a> {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: &'a Body,
        }
        encode(&Before { version: 1, id: MessageId::new(), sent_at: 1, body })
    }

    #[test]
    fn a_packet_from_an_app_before_the_stamp_says_nothing_of_the_mailbox() {
        let body = Body::Message { text: "hi".to_owned(), reply_to: None };
        let packet = Packet::decode(&packet_before_the_mailbox_stamp(&body)).expect("decodes");
        assert_eq!((packet.body, packet.mailbox), (body, None));
    }

    #[test]
    fn an_app_before_the_stamp_reads_a_packet_that_says_it() {
        #[derive(serde::Deserialize)]
        struct Before {
            version: u16,
            sent_at: u64,
            body: ciborium::Value,
        }
        let body = Body::Delivered { ids: vec![MessageId::new()] };
        let packet = Packet { mailbox: Some(true), ..Packet::new(body.clone()) };
        let before: Before = decode(&packet.encode()).expect("an app before reads it");
        assert_eq!((before.version, before.sent_at), (packet.version, packet.sent_at));
        assert_eq!(before.body.deserialized::<Body>().expect("and its body"), body);
    }

    #[test]
    fn every_packet_type_survives_the_wire() {
        round_trip(Body::Message { text: "hello".to_owned(), reply_to: None });
        round_trip(Body::Message { text: "yes".to_owned(), reply_to: Some(MessageId::new()) });
        round_trip(Body::Reaction { to: MessageId::new(), emoji: "👍".to_owned() });
        round_trip(Body::Reaction { to: MessageId::new(), emoji: String::new() });
        round_trip(Body::Edit { of: MessageId::new(), text: "other words".to_owned() });
        round_trip(Body::Delete { of: MessageId::new() });
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
        round_trip(Body::CallOffer { call, sdp: "v=0".to_owned(), video: true, media: CALL_MEDIA_VERSION });
        round_trip(Body::CallOffer { call, sdp: "v=0".to_owned(), video: false, media: 0 });
        round_trip(Body::CallAnswer { call, sdp: "v=0".to_owned(), media: CALL_MEDIA_VERSION });
        round_trip(Body::CallMedia { call, seq: 7, video: true, paused: false });
        round_trip(Body::CallMedia { call, seq: 8, video: true, paused: true });
        round_trip(Body::CallPresent { call, seq: 1, plugin: Some("com.flickertalk.board".to_owned()), file: None });
        round_trip(Body::CallPresent { call, seq: 2, plugin: Some("com.flickertalk.pdfviewer".to_owned()), file: Some(MessageId::new()) });
        round_trip(Body::CallPresent { call, seq: 3, plugin: None, file: None });
        for reason in [EndReason::Hangup, EndReason::Declined, EndReason::Busy, EndReason::Cancelled, EndReason::Failed] {
            round_trip(Body::CallEnd { call, reason });
        }
        round_trip(Body::MoveOffer { proof: [1; 32], key: [2; 32], size: 1 << 20, hash: [3; 32] });
        round_trip(Body::MoveRequest { from: 4, count: 16 });
        round_trip(Body::MoveChunk { index: 4, data: vec![5; 10] });
        round_trip(Body::MoveDone);
        round_trip(Body::CircleCard { card: vec![4, 5, 6] });
        round_trip(Body::CircleMessage { circle: "ab".repeat(16), text: "hello all".to_owned() });
        round_trip(Body::CircleLeave { circle: "ab".repeat(16) });
        round_trip(Body::PluginEvent { plugin: "com.example.board".to_owned(), data: vec![1, 2, 3] });
    }

    #[test]
    fn a_file_chunk_fits_one_data_channel_message() {
        let packet = Packet::new(Body::FileChunk { file: MessageId::new(), index: 1 << 40, data: vec![0xab; FILE_CHUNK as usize] });
        assert!(packet.encode().len() + 200 < 64 * 1024, "{} bytes", packet.encode().len());
    }

    #[test]
    fn message_ids_are_uuid_v7() {
        let id = MessageId::new();
        assert_eq!(id.as_uuid().get_version_num(), 7);
        assert_eq!(MessageId::parse(&id.to_string()).expect("parses"), id);
    }

    #[test]
    fn a_retried_packet_keeps_its_id_and_time() {
        let original = Packet::new(Body::Message { text: "hi".to_owned(), reply_to: None });
        let retry = Packet::resend(original.id, original.sent_at, original.body.clone());
        assert_eq!(retry, original);
    }

    #[test]
    fn packets_carry_the_protocol_version() {
        assert_eq!(Packet::new(Body::Ping).version, PROTOCOL_VERSION);
    }

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

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "type", content = "data", rename_all = "snake_case")]
    enum OlderBody {
        CallOffer { call: MessageId, sdp: String, video: bool },
        CallAnswer { call: MessageId, sdp: String },
    }

    fn older_packet(body: OlderBody) -> Vec<u8> {
        #[derive(serde::Serialize)]
        struct Older {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: OlderBody,
        }
        let mut bytes = Vec::new();
        ciborium::into_writer(&Older { version: 1, id: MessageId::new(), sent_at: 1, body }, &mut bytes).expect("encodes");
        bytes
    }

    fn read_as_older(packet: &Packet) -> Result<OlderBody, ciborium::value::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            body: ciborium::Value,
        }
        let raw: Raw = decode(&packet.encode()).expect("decodes");
        raw.body.deserialized()
    }

    #[test]
    fn a_call_offer_or_answer_without_a_media_version_reads_as_version_zero() {
        let call = MessageId::new();
        let offer = Packet::decode(&older_packet(OlderBody::CallOffer { call, sdp: "v=0".to_owned(), video: true })).expect("decodes");
        assert_eq!(offer.body, Body::CallOffer { call, sdp: "v=0".to_owned(), video: true, media: 0 });
        let answer = Packet::decode(&older_packet(OlderBody::CallAnswer { call, sdp: "v=0".to_owned() })).expect("decodes");
        assert_eq!(answer.body, Body::CallAnswer { call, sdp: "v=0".to_owned(), media: 0 });
    }

    #[test]
    fn an_older_app_reads_the_new_offer_and_answer_and_ignores_the_camera_state() {
        let call = MessageId::new();
        let offer = Packet::new(Body::CallOffer { call, sdp: "v=0".to_owned(), video: false, media: CALL_MEDIA_VERSION });
        assert_eq!(read_as_older(&offer).expect("an older app reads it"), OlderBody::CallOffer { call, sdp: "v=0".to_owned(), video: false });
        let answer = Packet::new(Body::CallAnswer { call, sdp: "v=0".to_owned(), media: CALL_MEDIA_VERSION });
        assert_eq!(read_as_older(&answer).expect("an older app reads it"), OlderBody::CallAnswer { call, sdp: "v=0".to_owned() });
        let camera = Packet::new(Body::CallMedia { call, seq: 1, video: true, paused: false });
        assert!(read_as_older(&camera).is_err(), "an older app decodes it as Unknown and ignores it");
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    #[serde(tag = "type", content = "data", rename_all = "snake_case")]
    enum MediaOneBody {
        CallOffer {
            call: MessageId,
            sdp: String,
            video: bool,
            #[serde(default)]
            media: u16,
        },
    }

    fn read_as_media_one(packet: &Packet) -> Result<MediaOneBody, ciborium::value::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            body: ciborium::Value,
        }
        let raw: Raw = decode(&packet.encode()).expect("decodes");
        raw.body.deserialized()
    }

    #[test]
    fn an_app_at_media_one_reads_a_media_two_offer_and_ignores_a_presentation() {
        assert_eq!((CALL_MEDIA_CAMERA, CALL_MEDIA_PRESENT, CALL_MEDIA_VERSION), (1, 2, 2));
        let call = MessageId::new();
        let offer = Packet::new(Body::CallOffer { call, sdp: "v=0".to_owned(), video: false, media: CALL_MEDIA_VERSION });
        assert_eq!(
            read_as_media_one(&offer).expect("an app at media 1 reads it"),
            MediaOneBody::CallOffer { call, sdp: "v=0".to_owned(), video: false, media: 2 }
        );
        let present = Packet::new(Body::CallPresent { call, seq: 1, plugin: Some("com.flickertalk.board".to_owned()), file: None });
        assert!(read_as_media_one(&present).is_err(), "an app at media 1 decodes it as Unknown");
        assert!(read_as_older(&present).is_err(), "and so does an app before");
    }

    #[test]
    fn a_presentation_is_padded_to_the_bucket() {
        let call = MessageId::new();
        for body in [
            Body::CallPresent { call, seq: u32::MAX, plugin: Some("com.flickertalk.pdfviewer".to_owned()), file: Some(MessageId::new()) },
            Body::CallPresent { call, seq: 1, plugin: None, file: None },
        ] {
            let packet = Packet::new(body);
            let bytes = packet.encode();
            assert_eq!(bytes.len() % PAD_BUCKET, 0, "{} bytes", bytes.len());
            assert_eq!(Packet::decode(&bytes).expect("decodes"), packet);
        }
    }

    #[test]
    fn a_camera_state_with_fields_from_a_newer_app_is_still_read() {
        #[derive(serde::Serialize)]
        #[serde(tag = "type", content = "data", rename_all = "snake_case")]
        enum NewerBody {
            CallMedia { call: MessageId, seq: u32, video: bool, paused: bool, screen: bool },
        }
        #[derive(serde::Serialize)]
        struct Newer {
            version: u16,
            id: MessageId,
            sent_at: u64,
            body: NewerBody,
        }
        let call = MessageId::new();
        let body = NewerBody::CallMedia { call, seq: 3, video: false, paused: false, screen: true };
        let mut bytes = Vec::new();
        ciborium::into_writer(&Newer { version: 2, id: MessageId::new(), sent_at: 1, body }, &mut bytes).expect("encodes");
        assert_eq!(Packet::decode(&bytes).expect("decodes").body, Body::CallMedia { call, seq: 3, video: false, paused: false });
    }

    #[test]
    fn packets_are_padded_to_the_bucket() {
        for text in ["ok", "a somewhat longer answer, say", &"x".repeat(1000)] {
            let packet = Packet::new(Body::Message { text: text.to_owned(), reply_to: None });
            let bytes = packet.encode();
            assert_eq!(bytes.len() % PAD_BUCKET, 0, "{} bytes for {} chars", bytes.len(), text.len());
            assert_eq!(Packet::decode(&bytes).expect("decodes"), packet, "the pad is dropped on the way in");
        }
        let short = Packet::new(Body::Message { text: "ok".to_owned(), reply_to: None }).encode().len();
        let longer = Packet::new(Body::Message { text: "a somewhat longer answer, say".to_owned(), reply_to: None }).encode().len();
        assert_eq!(short, longer, "two short texts weigh the same");
    }

    #[test]
    fn envelopes_survive_the_wire_and_are_recognised() {
        let envelope = Envelope { envelope: ENVELOPE_VERSION, sealed: vec![7; 48] };
        assert_eq!(Envelope::decode(&envelope.encode()).expect("decodes"), envelope);
        assert!(Envelope::is_envelope(&envelope.encode()));
        let bare = Sealed { version: PROTOCOL_VERSION, from: "ft_x".to_owned(), kind: SealedKind::Normal, ciphertext: vec![1] };
        assert!(!Envelope::is_envelope(&bare.encode()));
        assert!(!Envelope::is_envelope(b"\xff\x00"));
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

    #[test]
    fn ciphertext_is_encoded_as_raw_bytes() {
        let sealed = Sealed { version: 1, from: String::new(), kind: SealedKind::Normal, ciphertext: vec![0; 1000] };
        assert!(sealed.encode().len() < 1100);
    }
}
