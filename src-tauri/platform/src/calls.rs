use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
struct Ringing<'a> {
    caller: &'a str,
    video: bool,
    muted: bool,
}

#[derive(Serialize)]
struct Answering<'a> {
    caller: &'a str,
    video: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCallEvent {
    Incoming,
    Answer,
    End,
    Decline,
    Mute(bool),
    AudioActivated(u64),
    AudioDeactivated(u64),
    Visible(bool),
    /// iOS sends `UIDeviceOrientation.rawValue`, Android the display rotation in degrees
    Orientation(i32),
    VideoRequested,
    Route(CallRoute),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallRoute {
    Receiver,
    Speaker,
    Headset,
}

impl CallRoute {
    fn parse(wire: &str) -> Option<Self> {
        match wire {
            "receiver" => Some(Self::Receiver),
            "speaker" => Some(Self::Speaker),
            "headset" => Some(Self::Headset),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receiver => "receiver",
            Self::Speaker => "speaker",
            Self::Headset => "headset",
        }
    }
}

#[derive(Deserialize)]
struct CallEventWire {
    event: String,
    muted: Option<bool>,
    #[serde(default)]
    generation: u64,
    visible: Option<bool>,
    orientation: Option<i32>,
    route: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn call_event(body: tauri::ipc::InvokeResponseBody) -> Option<NativeCallEvent> {
    call_event_of(body.deserialize().ok()?)
}

pub fn parse_call_event(json: &str) -> Option<NativeCallEvent> {
    call_event_of(serde_json::from_str(json).ok()?)
}

fn call_event_of(wire: CallEventWire) -> Option<NativeCallEvent> {
    match (wire.event.as_str(), wire.muted) {
        ("visible", _) => wire.visible.map(NativeCallEvent::Visible),
        ("orientation", _) => wire.orientation.map(NativeCallEvent::Orientation),
        ("video", _) => Some(NativeCallEvent::VideoRequested),
        ("incoming", _) => Some(NativeCallEvent::Incoming),
        ("answer", _) => Some(NativeCallEvent::Answer),
        ("end", _) => Some(NativeCallEvent::End),
        ("decline", _) => Some(NativeCallEvent::Decline),
        ("mute", Some(muted)) => Some(NativeCallEvent::Mute(muted)),
        ("audioActivated", _) => Some(NativeCallEvent::AudioActivated(wire.generation)),
        ("audioDeactivated", _) => Some(NativeCallEvent::AudioDeactivated(wire.generation)),
        ("route", _) => wire
            .route
            .as_deref()
            .and_then(CallRoute::parse)
            .map(NativeCallEvent::Route),
        _ => None,
    }
}

// Created on the Rust side so call events still arrive with no WebView, e.g. on a locked iPhone.
#[cfg_attr(not(mobile), allow(dead_code))]
fn call_channel(
    handler: impl Fn(NativeCallEvent) + Send + Sync + 'static,
) -> tauri::ipc::Channel<serde_json::Value> {
    tauri::ipc::Channel::new(move |body| {
        if let Some(event) = call_event(body) {
            handler(event);
        }
        Ok(())
    })
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct CallEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[derive(Serialize)]
struct Speaker {
    on: bool,
}

#[derive(Serialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct OpenAppOnAnswer {
    on: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct Answered {
    answered: bool,
}

#[derive(Serialize)]
struct Outgoing<'a> {
    name: &'a str,
    video: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Microphone {
    granted: bool,
}

impl<R: Runtime> Platform<R> {
    pub fn start_ringing(&self, caller: &str, video: bool, muted: bool) -> Result<()> {
        self.run(
            "startRinging",
            Ringing {
                caller,
                video,
                muted,
            },
        )
    }

    pub fn stop_ringing(&self) -> Result<()> {
        self.run("stopRinging", ())
    }

    pub fn call_refused(&self) -> Result<()> {
        self.call("callRefused", ())
    }

    pub fn call_answering(&self, caller: &str, video: bool) -> Result<()> {
        self.call("callAnswering", Answering { caller, video })
    }

    /// The handler runs on a native background thread, so it must not block for long.
    pub fn listen_calls(&self, handler: impl Fn(NativeCallEvent) + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = call_channel(handler);
            let _ = self.run("registerCallEvents", CallEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    pub fn call_started_outgoing(&self, name: &str, video: bool) -> Result<()> {
        self.call("callStartedOutgoing", Outgoing { name, video })
    }

    pub fn call_connected(&self) -> Result<()> {
        self.call("callConnected", ())
    }

    /// false when the OS has no call to answer, then the core answers by itself
    pub fn answer_call(&self) -> Result<bool> {
        #[cfg(target_os = "ios")]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Answered>("answerCall", ())?
                .answered)
        }
        #[cfg(not(target_os = "ios"))]
        {
            Ok(false)
        }
    }

    pub fn set_speaker(&self, on: bool) -> Result<()> {
        self.call("setSpeaker", Speaker { on })
    }

    pub fn call_ended(&self) -> Result<()> {
        self.call("callEnded", ())
    }

    /// iOS only opens the app after an answer for video calls, so with `on` every incoming call
    /// is reported to CallKit as video. The call's real media doesn't change.
    pub fn set_open_app_on_answer(&self, on: bool) -> Result<()> {
        #[cfg(target_os = "ios")]
        {
            self.run("setOpenAppOnAnswer", OpenAppOnAnswer { on })
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = on;
            Ok(())
        }
    }

    pub fn request_microphone(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Microphone>("requestMicrophone", ())?
                .granted)
        }
        #[cfg(not(mobile))]
        {
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kotlin_side_gets_the_ringing_names_it_expects() {
        let ring = serde_json::to_value(Ringing {
            caller: "Alice",
            video: true,
            muted: true,
        })
        .unwrap();
        assert_eq!(
            ring,
            serde_json::json!({ "caller": "Alice", "video": true, "muted": true })
        );
    }

    #[test]
    fn native_call_events_are_read_from_their_wire_form() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"answer"}"#), Some(NativeCallEvent::Answer));
        assert_eq!(
            read(r#"{"event":"incoming"}"#),
            Some(NativeCallEvent::Incoming)
        );
        assert_eq!(read(r#"{"event":"end"}"#), Some(NativeCallEvent::End));
        assert_eq!(
            read(r#"{"event":"mute","muted":true}"#),
            Some(NativeCallEvent::Mute(true))
        );
        assert_eq!(
            read(r#"{"event":"mute","muted":false}"#),
            Some(NativeCallEvent::Mute(false))
        );
        assert_eq!(
            read(r#"{"event":"audioActivated","generation":3}"#),
            Some(NativeCallEvent::AudioActivated(3))
        );
        assert_eq!(
            read(r#"{"event":"audioDeactivated","generation":2}"#),
            Some(NativeCallEvent::AudioDeactivated(2))
        );
        assert_eq!(
            read(r#"{"event":"audioActivated"}"#),
            Some(NativeCallEvent::AudioActivated(0))
        );
    }

    #[test]
    fn a_decline_and_an_answered_call_travel_as_the_native_side_says_them() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(
            read(r#"{"event":"decline"}"#),
            Some(NativeCallEvent::Decline)
        );
        let answering = serde_json::to_value(Answering {
            caller: "Alice",
            video: true,
        })
        .unwrap();
        assert_eq!(
            answering,
            serde_json::json!({ "caller": "Alice", "video": true })
        );
    }

    #[test]
    fn a_call_event_is_read_from_the_text_kotlin_hands_over() {
        assert_eq!(
            parse_call_event(r#"{"event":"decline"}"#),
            Some(NativeCallEvent::Decline)
        );
        assert_eq!(
            parse_call_event(r#"{"event":"end"}"#),
            Some(NativeCallEvent::End)
        );
        assert_eq!(
            parse_call_event(r#"{"event":"incoming"}"#),
            Some(NativeCallEvent::Incoming)
        );
        assert_eq!(
            parse_call_event(r#"{"event":"mute","muted":true}"#),
            Some(NativeCallEvent::Mute(true))
        );
        assert_eq!(parse_call_event(r#"{"event":"hold"}"#), None);
        assert_eq!(parse_call_event("not json"), None);
    }

    #[test]
    fn the_call_route_travels_as_the_native_side_says_it() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(
            read(r#"{"event":"route","route":"headset"}"#),
            Some(NativeCallEvent::Route(CallRoute::Headset))
        );
        assert_eq!(
            read(r#"{"event":"route","route":"speaker"}"#),
            Some(NativeCallEvent::Route(CallRoute::Speaker))
        );
        assert_eq!(
            read(r#"{"event":"route","route":"receiver"}"#),
            Some(NativeCallEvent::Route(CallRoute::Receiver))
        );
        assert_eq!(read(r#"{"event":"route","route":"car"}"#), None);
        assert_eq!(read(r#"{"event":"route"}"#), None);
        assert_eq!(
            parse_call_event(r#"{"event":"route","route":"headset"}"#),
            Some(NativeCallEvent::Route(CallRoute::Headset))
        );
        assert_eq!(CallRoute::Headset.as_str(), "headset");
        assert_eq!(CallRoute::Speaker.as_str(), "speaker");
        assert_eq!(CallRoute::Receiver.as_str(), "receiver");
    }

    #[test]
    fn unknown_or_broken_call_events_are_ignored() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"hold"}"#), None);
        assert_eq!(read(r#"{"event":"mute"}"#), None);
        assert_eq!(read("not json"), None);
        assert_eq!(
            call_event(tauri::ipc::InvokeResponseBody::Raw(vec![1, 2])),
            None
        );
    }

    #[test]
    fn the_call_channel_goes_native_and_its_events_reach_the_handler() {
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = heard.clone();
        let channel = call_channel(move |event| sink.lock().unwrap().push(event));
        let args = serde_json::to_value(CallEvents { channel: &channel }).unwrap();
        assert_eq!(
            args,
            serde_json::json!({ "channel": format!("__CHANNEL__:{}", channel.id()) })
        );
        channel
            .send(serde_json::json!({ "event": "answer" }))
            .unwrap();
        channel
            .send(serde_json::json!({ "event": "mute", "muted": true }))
            .unwrap();
        channel
            .send(serde_json::json!({ "event": "nonsense" }))
            .unwrap();
        assert_eq!(
            *heard.lock().unwrap(),
            vec![NativeCallEvent::Answer, NativeCallEvent::Mute(true)]
        );
    }

    #[test]
    fn the_speaker_and_the_answer_travel_as_swift_and_kotlin_read_them() {
        assert_eq!(
            serde_json::to_value(Speaker { on: true }).unwrap(),
            serde_json::json!({ "on": true })
        );
        let answered: Answered =
            serde_json::from_value(serde_json::json!({ "answered": true })).unwrap();
        assert!(answered.answered);
    }

    #[test]
    fn native_video_events_are_read_from_their_wire_form() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(
            read(r#"{"event":"visible","visible":true}"#),
            Some(NativeCallEvent::Visible(true))
        );
        assert_eq!(
            read(r#"{"event":"visible","visible":false}"#),
            Some(NativeCallEvent::Visible(false))
        );
        assert_eq!(
            read(r#"{"event":"orientation","orientation":3}"#),
            Some(NativeCallEvent::Orientation(3))
        );
        assert_eq!(
            read(r#"{"event":"video"}"#),
            Some(NativeCallEvent::VideoRequested)
        );
        assert_eq!(read(r#"{"event":"visible"}"#), None);
        assert_eq!(read(r#"{"event":"orientation"}"#), None);
    }

    #[test]
    fn the_open_app_on_answer_switch_travels_as_swift_reads_it() {
        assert_eq!(
            serde_json::to_value(OpenAppOnAnswer { on: true }).unwrap(),
            serde_json::json!({ "on": true })
        );
        assert_eq!(
            serde_json::to_value(OpenAppOnAnswer { on: false }).unwrap(),
            serde_json::json!({ "on": false })
        );
    }

    #[test]
    fn an_outgoing_call_names_the_contact() {
        let outgoing = serde_json::to_value(Outgoing {
            name: "Alice",
            video: false,
        })
        .unwrap();
        assert_eq!(
            outgoing,
            serde_json::json!({ "name": "Alice", "video": false })
        );
    }
}
