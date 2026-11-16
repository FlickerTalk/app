use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachVideo {
    remote_layer: u64,
    local_layer: u64,
}

/// CSS px, from getBoundingClientRect()
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VideoRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoLayout {
    pub remote: Option<VideoRect>,
    pub local: Option<VideoRect>,
    pub mirror_local: bool,
    pub local_radius: f64,
}

#[derive(Serialize)]
struct VideoShape {
    width: u32,
    height: u32,
    rotation: u16,
}

#[derive(Serialize)]
struct CallVideo {
    on: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Camera {
    granted: bool,
}

impl<R: Runtime> Platform<R> {
    /// Layer addresses on iOS (`ft_media::views::layers`). Android passes 0 for both and Kotlin
    /// hands the surfaces over JNI instead.
    pub fn attach_video(&self, remote_layer: usize, local_layer: usize) -> Result<()> {
        self.call(
            "attachVideo",
            AttachVideo {
                remote_layer: remote_layer as u64,
                local_layer: local_layer as u64,
            },
        )
    }

    pub fn video_layout(&self, layout: &VideoLayout) -> Result<()> {
        self.call("videoLayout", layout)
    }

    pub fn video_shape(&self, width: u32, height: u32, rotation: u16) -> Result<()> {
        self.call(
            "videoShape",
            VideoShape {
                width,
                height,
                rotation,
            },
        )
    }

    /// On iOS this returns once the layers are out of the view hierarchy. Only then may the call's
    /// video devices go.
    pub fn detach_video(&self) -> Result<()> {
        self.call("detachVideo", ())
    }

    pub fn call_video(&self, on: bool) -> Result<()> {
        self.call("callVideo", CallVideo { on })
    }

    pub fn request_camera(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Camera>("requestCamera", ())?
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
    fn native_video_travels_as_swift_and_kotlin_read_it() {
        let attach = serde_json::to_value(AttachVideo {
            remote_layer: 0x1000,
            local_layer: 0x2000,
        })
        .unwrap();
        assert_eq!(
            attach,
            serde_json::json!({ "remoteLayer": 4096, "localLayer": 8192 })
        );
        let layout = VideoLayout {
            remote: Some(VideoRect {
                x: 0.0,
                y: 0.0,
                width: 390.0,
                height: 844.0,
            }),
            local: Some(VideoRect {
                x: 278.0,
                y: 594.0,
                width: 96.0,
                height: 140.0,
            }),
            mirror_local: true,
            local_radius: 16.0,
        };
        assert_eq!(
            serde_json::to_value(layout).unwrap(),
            serde_json::json!({
                "remote": { "x": 0.0, "y": 0.0, "width": 390.0, "height": 844.0 },
                "local": { "x": 278.0, "y": 594.0, "width": 96.0, "height": 140.0 },
                "mirrorLocal": true,
                "localRadius": 16.0
            })
        );
        let hidden: VideoLayout = serde_json::from_value(serde_json::json!({ "remote": null, "local": null, "mirrorLocal": false, "localRadius": 0 })).unwrap();
        assert_eq!((hidden.remote, hidden.local), (None, None));
        let shape = serde_json::to_value(VideoShape {
            width: 480,
            height: 640,
            rotation: 90,
        })
        .unwrap();
        assert_eq!(
            shape,
            serde_json::json!({ "width": 480, "height": 640, "rotation": 90 })
        );
        assert_eq!(
            serde_json::to_value(CallVideo { on: true }).unwrap(),
            serde_json::json!({ "on": true })
        );
        let camera: Camera =
            serde_json::from_value(serde_json::json!({ "granted": false })).unwrap();
        assert!(!camera.granted);
    }
}
