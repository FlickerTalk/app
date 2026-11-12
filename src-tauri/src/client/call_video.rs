use std::sync::{Mutex, PoisonError};

use ft_media::{Facing, Layers, RemoteShape, VideoState};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::{PlatformExt, VideoLayout};

use super::Client;

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeVideo {
    CallVideo(bool),
    Attach(Layers),
    Layout(VideoLayout),
    Shape(RemoteShape),
    Detach,
}

pub const HIDDEN: VideoLayout = VideoLayout { remote: None, local: None, mirror_local: false, local_radius: 0.0 };

#[derive(Default)]
pub struct VideoGlue {
    call: Option<String>,
    ended: Option<String>,
    has_video: bool,
    attached: bool,
    shape: Option<RemoteShape>,
    layout: Option<VideoLayout>,
    facing: Facing,
}

impl VideoGlue {
    pub fn state(&mut self, call: &str, state: &VideoState, layers: Option<Layers>) -> Vec<BridgeVideo> {
        if self.ended.as_deref() == Some(call) {
            return Vec::new();
        }
        if self.call.as_deref() != Some(call) {
            *self = Self { call: Some(call.to_owned()), layout: self.layout, ..Self::default() };
        }
        let mut told = Vec::new();
        if state.any() != self.has_video {
            self.has_video = state.any();
            told.push(BridgeVideo::CallVideo(self.has_video));
        }
        let turned = state.facing != self.facing;
        self.facing = state.facing;
        if !self.attached && state.any() {
            if let Some(layers) = layers {
                self.attached = true;
                told.push(BridgeVideo::Attach(layers));
                told.extend(self.placed().map(BridgeVideo::Layout));
            }
        } else if self.attached && turned {
            told.extend(self.placed().map(BridgeVideo::Layout));
        }
        if self.attached && state.shape != self.shape {
            self.shape = state.shape;
            told.extend(state.shape.map(BridgeVideo::Shape));
        }
        told
    }

    pub fn layout(&mut self, layout: Option<VideoLayout>) -> Vec<BridgeVideo> {
        self.layout = layout;
        if self.attached {
            vec![BridgeVideo::Layout(self.placed().unwrap_or(HIDDEN))]
        } else {
            Vec::new()
        }
    }

    fn placed(&self) -> Option<VideoLayout> {
        self.layout.map(|layout| VideoLayout { mirror_local: self.facing == Facing::Front, ..layout })
    }

    pub fn end(&mut self) -> Vec<BridgeVideo> {
        let attached = self.attached;
        *self = Self { ended: self.call.take().or(self.ended.take()), layout: self.layout, ..Self::default() };
        if attached {
            vec![BridgeVideo::Detach]
        } else {
            Vec::new()
        }
    }
}

pub(super) fn video_layers() -> Option<Layers> {
    if cfg!(target_os = "ios") {
        ft_media::views::layers()
    } else {
        Some(Layers { remote: 0, local: 0 })
    }
}

/// Holds the glue's lock while the bridge is told, so the views never come back after they went.
pub(super) fn tell_bridge(app: &AppHandle, glue: &Mutex<VideoGlue>, step: impl FnOnce(&mut VideoGlue) -> Vec<BridgeVideo>) {
    let mut glue = glue.lock().unwrap_or_else(PoisonError::into_inner);
    for told in step(&mut glue) {
        let platform = app.platform();
        let _ = match told {
            BridgeVideo::CallVideo(on) => platform.call_video(on),
            BridgeVideo::Attach(layers) => platform.attach_video(layers.remote, layers.local),
            BridgeVideo::Layout(layout) => platform.video_layout(&layout),
            BridgeVideo::Shape(shape) => platform.video_shape(shape.width, shape.height, shape.rotation),
            BridgeVideo::Detach => platform.detach_video(),
        };
    }
}

#[tauri::command]
pub async fn core_call_video_layout(layout: Option<VideoLayout>, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let glue = client.video.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || tell_bridge(&app, &glue, |glue| glue.layout(layout))).await;
    if let Ok(core) = client.core().await {
        let _ = core.set_call_shown(layout.is_some()).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tauri_plugin_ft_platform::VideoRect;

    use super::*;

    fn laid_out() -> VideoLayout {
        VideoLayout {
            remote: Some(VideoRect { x: 0.0, y: 0.0, width: 390.0, height: 844.0 }),
            local: Some(VideoRect { x: 278.0, y: 594.0, width: 96.0, height: 140.0 }),
            mirror_local: true,
            local_radius: 16.0,
        }
    }

    #[test]
    fn the_webview_s_layout_is_read_as_it_sends_it() {
        let sent = serde_json::json!({
            "remote": { "x": 0, "y": 0, "width": 390, "height": 844 },
            "local": { "x": 278, "y": 594, "width": 96, "height": 140 },
            "mirrorLocal": true,
            "localRadius": 16
        });
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(sent).unwrap(), Some(laid_out()));
        let voice = serde_json::json!({ "remote": null, "local": null, "mirrorLocal": false, "localRadius": 0 });
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(voice).unwrap(), Some(HIDDEN));
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(serde_json::Value::Null).unwrap(), None, "left the call screen");
    }

    const LAYERS: Layers = Layers { remote: 0x10, local: 0x20 };

    #[test]
    fn the_bridge_attaches_the_views_once_the_call_has_video_and_follows_it() {
        let mut glue = VideoGlue::default();
        let voice = VideoState { available: true, ..VideoState::default() };
        assert_eq!(glue.state("c1", &voice, Some(LAYERS)), vec![], "a voice call shows nothing");
        assert_eq!(glue.layout(Some(laid_out())), vec![], "laid out before there is video: kept for then");
        let theirs = VideoState { remote: true, ..voice };
        assert_eq!(
            glue.state("c1", &theirs, Some(LAYERS)),
            vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS), BridgeVideo::Layout(laid_out())]
        );
        assert_eq!(glue.state("c1", &VideoState { camera: true, ..theirs }, Some(LAYERS)), vec![], "attached once, and CallKit knows");
        let shape = RemoteShape { width: 640, height: 480, rotation: 90 };
        let shaped = VideoState { shape: Some(shape), ..theirs };
        assert_eq!(glue.state("c1", &shaped, Some(LAYERS)), vec![BridgeVideo::Shape(shape)]);
        assert_eq!(glue.state("c1", &shaped, Some(LAYERS)), vec![], "the same shape again");
        let moved = VideoLayout { local: None, ..laid_out() };
        assert_eq!(glue.layout(Some(moved)), vec![BridgeVideo::Layout(moved)]);
        assert_eq!(glue.layout(None), vec![BridgeVideo::Layout(HIDDEN)], "off the call screen the views hide");
        let voice_again = VideoState { shape: Some(shape), ..voice };
        assert_eq!(glue.state("c1", &voice_again, Some(LAYERS)), vec![BridgeVideo::CallVideo(false)], "the views stay until the end");
        assert_eq!(glue.end(), vec![BridgeVideo::Detach]);
        assert_eq!(glue.state("c1", &theirs, Some(LAYERS)), vec![], "a late state of the call that ended");
        assert_eq!(glue.end(), vec![], "taken away once");
    }

    #[test]
    fn the_views_wait_for_the_call_s_layers() {
        let mut glue = VideoGlue::default();
        let theirs = VideoState { remote: true, ..VideoState::default() };
        assert_eq!(glue.state("c1", &theirs, None), vec![BridgeVideo::CallVideo(true)]);
        assert_eq!(glue.state("c1", &theirs, Some(LAYERS)), vec![BridgeVideo::Attach(LAYERS)], "nothing laid out yet");
        assert_eq!(glue.end(), vec![BridgeVideo::Detach]);
        assert_eq!(glue.state("c2", &theirs, Some(LAYERS)), vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS)]);
    }

    #[test]
    fn our_preview_is_mirrored_only_with_the_front_camera() {
        let mut glue = VideoGlue::default();
        let front = VideoState { available: true, camera: true, ..VideoState::default() };
        let unmirrored = VideoLayout { mirror_local: false, ..laid_out() };
        assert_eq!(glue.layout(Some(unmirrored)), vec![]);
        assert_eq!(
            glue.state("c1", &front, Some(LAYERS)),
            vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS), BridgeVideo::Layout(laid_out())],
            "the front camera is mirrored"
        );
        let back = VideoState { facing: Facing::Back, ..front };
        assert_eq!(glue.state("c1", &back, Some(LAYERS)), vec![BridgeVideo::Layout(unmirrored)], "flipped to the back: not mirrored, at once");
        assert_eq!(glue.layout(Some(laid_out())), vec![BridgeVideo::Layout(unmirrored)], "a late layout from before the flip");
        assert_eq!(glue.state("c1", &front, Some(LAYERS)), vec![BridgeVideo::Layout(laid_out())], "and back to the front");
        assert_eq!(glue.layout(None), vec![BridgeVideo::Layout(HIDDEN)]);
        assert_eq!(glue.state("c1", &back, Some(LAYERS)), vec![], "hidden: nothing to place");
        assert_eq!(glue.layout(Some(laid_out())), vec![BridgeVideo::Layout(unmirrored)], "shown again with the back camera");
    }

    #[test]
    fn a_call_without_video_has_no_views_to_take_away() {
        let mut glue = VideoGlue::default();
        assert_eq!(glue.state("c1", &VideoState { available: true, ..VideoState::default() }, Some(LAYERS)), vec![]);
        assert_eq!(glue.end(), vec![]);
    }
}
