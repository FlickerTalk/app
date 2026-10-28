//! The media the support contact carries inside its binary (made by `assets/make-assets.sh`):
//! the sample file and the voice message it sends back, and what a call hears and sees.

use anyhow::{anyhow, ensure, Context, Result};
use webrtc_engine::codec::Decoder;
use webrtc_engine::video::h264::{nal_type, nal_units, NAL_IDR, NAL_SEI, NAL_SLICE, START_CODE};

/// Sent back for a file: the app's icon.
pub const SAMPLE_FILE: &[u8] = include_bytes!("../assets/sample.png");
pub const SAMPLE_NAME: &str = "flickertalk-sample.png";
pub const SAMPLE_MIME: &str = "image/png";

/// Sent back for a voice message: AAC in MP4, as the app records it.
pub const VOICE_MESSAGE: &[u8] = include_bytes!("../assets/voice.m4a");
pub const VOICE_MIME: &str = "audio/mp4";

/// What a call hears, on a loop: Ogg Opus, 48 kHz mono.
const CALL_VOICE: &[u8] = include_bytes!("../assets/call.opus");

/// What a call sees, on a loop: H.264 Constrained Baseline in Annex-B, at `CALL_FPS`.
const CALL_VIDEO: &[u8] = include_bytes!("../assets/call.h264");
pub const CALL_FPS: u32 = 15;

/// The name a voice message gets, as the app names its own (`voice-YYYYMMDD-HHMMSS.m4a`), at
/// `unix_seconds` (UTC).
pub fn voice_name(unix_seconds: u64) -> String {
    let (days, seconds) = (unix_seconds / 86_400, unix_seconds % 86_400);
    let (year, month, day) = civil_date(days);
    format!("voice-{year:04}{month:02}{day:02}-{:02}{:02}{:02}.m4a", seconds / 3_600, seconds % 3_600 / 60, seconds % 60)
}

/// The date of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_date(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// The packets of an Ogg stream, in order.
pub fn ogg_packets(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    const HEADER: usize = 27;
    ensure!(!bytes.is_empty(), "an empty Ogg stream");
    let (mut packets, mut partial, mut at) = (Vec::new(), Vec::new(), 0);
    while at < bytes.len() {
        let header = bytes.get(at..at + HEADER).context("a cut Ogg page")?;
        ensure!(header.starts_with(b"OggS"), "not an Ogg stream");
        let lacing = bytes.get(at + HEADER..at + HEADER + usize::from(header[26])).context("a cut Ogg page")?;
        let mut data = at + HEADER + lacing.len();
        for &length in lacing {
            let segment = bytes.get(data..data + usize::from(length)).context("a cut Ogg page")?;
            partial.extend_from_slice(segment);
            data += segment.len();
            // A segment shorter than 255 bytes ends its packet; one of 255 goes on in the next.
            if length < 255 {
                packets.push(std::mem::take(&mut partial));
            }
        }
        at = data;
    }
    Ok(packets)
}

/// The 48 kHz mono samples of an Ogg Opus stream, without its pre-skip.
pub fn decode_opus(bytes: &[u8]) -> Result<Vec<i16>> {
    let packets = ogg_packets(bytes)?;
    let head = packets.first().context("an empty Opus stream")?;
    ensure!(head.starts_with(b"OpusHead") && head.len() >= 19, "not an Ogg Opus stream");
    ensure!(head[9] == 1, "the call's voice is mono");
    let pre_skip = usize::from(u16::from_le_bytes([head[10], head[11]]));
    let mut decoder = Decoder::new().map_err(|error| anyhow!("no Opus decoder: {error:?}"))?;
    let mut samples = Vec::new();
    // After the head and the tags, every packet is sound.
    for packet in packets.iter().skip(2) {
        samples.extend(decoder.decode(packet).map_err(|error| anyhow!("a broken Opus packet: {error:?}"))?);
    }
    Ok(samples.split_off(pre_skip.min(samples.len())))
}

/// The voice a call hears.
pub fn call_voice() -> Result<Vec<i16>> {
    decode_opus(CALL_VOICE)
}

/// One whole access unit in Annex-B, as a camera's encoder hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// The access units of an Annex-B stream with one slice per frame: each slice with the units
/// before it (SPS, PPS). SEI units are left out: an encoder's banner, which no decoder needs.
pub fn access_units(stream: &[u8]) -> Vec<Frame> {
    let (mut frames, mut pending) = (Vec::new(), Vec::new());
    for unit in nal_units(stream).into_iter().filter(|unit| !unit.is_empty() && nal_type(unit[0]) != NAL_SEI) {
        pending.extend_from_slice(&START_CODE);
        pending.extend_from_slice(unit);
        let kind = nal_type(unit[0]);
        if kind == NAL_SLICE || kind == NAL_IDR {
            frames.push(Frame { data: std::mem::take(&mut pending), keyframe: kind == NAL_IDR });
        }
    }
    frames
}

/// The frames a video call sees.
pub fn call_video() -> Vec<Frame> {
    access_units(CALL_VIDEO)
}

#[cfg(test)]
mod tests {
    use webrtc_engine::video::h264::{NAL_PPS, NAL_SPS};
    use webrtc_engine::SAMPLE_RATE;

    use super::*;

    /// One Ogg page holding `packets`, each under 255 bytes unless the lacing says otherwise.
    fn page(segments: &[&[u8]], lacing: &[u8], continued: bool) -> Vec<u8> {
        let mut page = b"OggS".to_vec();
        page.push(0);
        page.push(u8::from(continued));
        page.extend_from_slice(&[0; 8 + 4 + 4 + 4]);
        page.push(lacing.len() as u8);
        page.extend_from_slice(lacing);
        for segment in segments {
            page.extend_from_slice(segment);
        }
        page
    }

    #[test]
    fn ogg_packets_follow_the_lacing_also_across_pages() {
        let long = vec![9u8; 300];
        let mut stream = page(&[b"ab", &long[..255]], &[2, 255], false);
        stream.extend(page(&[&long[255..], b"z"], &[45, 1], true));
        let packets = ogg_packets(&stream).expect("reads");
        assert_eq!(packets, vec![b"ab".to_vec(), long, b"z".to_vec()]);
    }

    #[test]
    fn something_that_is_not_ogg_is_refused() {
        assert!(ogg_packets(b"RIFF....WAVE").is_err());
        assert!(ogg_packets(&page(&[b"ab"], &[3], false)).is_err(), "a page shorter than its lacing");
    }

    #[test]
    fn the_call_voice_decodes_to_half_a_minute_or_so_of_sound() {
        // English, then Spanish (2026-10-28): some 30 to 40 seconds before it loops.
        let voice = call_voice().expect("decodes");
        let seconds = voice.len() as f64 / f64::from(SAMPLE_RATE);
        assert!((25.0..50.0).contains(&seconds), "{seconds} s");
        let loud = voice.iter().filter(|sample| sample.unsigned_abs() > 1_000).count();
        assert!(loud > voice.len() / 20, "it is speech, not silence");
    }

    #[test]
    fn an_opus_stream_must_start_with_its_head() {
        let stream = page(&[b"OpusTags"], &[8], false);
        assert!(decode_opus(&stream).is_err());
    }

    #[test]
    fn the_call_video_is_whole_frames_starting_with_a_keyframe_and_its_parameters() {
        let frames = call_video();
        assert_eq!(frames.len(), 60, "four seconds at 15 fps");
        let first = &frames[0];
        assert!(first.keyframe);
        let kinds: Vec<u8> = nal_units(&first.data).iter().map(|unit| nal_type(unit[0])).collect();
        assert_eq!(&kinds[kinds.len() - 3..], &[NAL_SPS, NAL_PPS, NAL_IDR], "{kinds:?}");
        assert_eq!(frames.iter().filter(|frame| frame.keyframe).count(), 4, "one a second");
        for frame in &frames {
            let slices = nal_units(&frame.data).iter().filter(|unit| matches!(nal_type(unit[0]), NAL_SLICE | NAL_IDR)).count();
            assert_eq!(slices, 1);
        }
    }

    #[test]
    fn access_units_split_at_each_slice() {
        let stream = [
            &[0, 0, 0, 1, 0x67, 1][..],
            &[0, 0, 0, 1, 0x68, 2],
            &[0, 0, 1, 0x65, 3],
            &[0, 0, 0, 1, 0x41, 4],
            &[0, 0, 0, 1, 0x41, 5],
        ]
        .concat();
        let frames = access_units(&stream);
        assert_eq!(frames.len(), 3);
        assert!(frames[0].keyframe && !frames[1].keyframe && !frames[2].keyframe);
        assert_eq!(nal_units(&frames[0].data).len(), 3);
        assert_eq!(nal_units(&frames[2].data), vec![&[0x41, 5][..]]);
    }

    #[test]
    fn the_encoder_s_banner_is_left_out() {
        let stream = [&[0, 0, 0, 1, 0x67, 1][..], &[0, 0, 0, 1, 0x68, 2], &[0, 0, 0, 1, 0x06, 9, 9, 9], &[0, 0, 0, 1, 0x65, 3]].concat();
        let frames = access_units(&stream);
        let kinds: Vec<u8> = nal_units(&frames[0].data).iter().map(|unit| nal_type(unit[0])).collect();
        assert_eq!(kinds, vec![NAL_SPS, NAL_PPS, NAL_IDR]);
    }

    #[test]
    fn the_samples_are_what_the_app_sends() {
        assert!(SAMPLE_FILE.starts_with(b"\x89PNG"));
        assert_eq!(&VOICE_MESSAGE[4..8], b"ftyp", "an MP4 container");
        assert!(SAMPLE_FILE.len() < 64 * 1024 && VOICE_MESSAGE.len() < 256 * 1024, "small enough for any line");
    }

    #[test]
    fn a_voice_message_is_named_as_the_app_names_its_own() {
        assert_eq!(voice_name(1_793_190_896), "voice-20261028-123456.m4a");
        assert_eq!(voice_name(0), "voice-19700101-000000.m4a");
    }
}
