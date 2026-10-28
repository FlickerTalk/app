#!/usr/bin/env bash
# Rebuilds the media the support peer carries inside its binary (`src/clips.rs`) and the app's
# icon. Run it from anywhere; it needs an ffmpeg with libx264 and libopus. Commit the four files it
# writes.
#
# The two recordings (English, then Spanish) are spoken by Kokoro, a local neural TTS under the
# Apache 2.0 licence, in the voices of the user manual's narration: make_voice.py says what and
# how. It needs a Python with kokoro_onnx, numpy and soundfile (FT_KOKORO_PYTHON, python3 by
# default) and the model and voices (FT_KOKORO_MODEL, FT_KOKORO_VOICES; make_voice.py has the
# defaults). Without them, espeak-ng speaks the English lines alone, as a last resort. Never the
# voices of macOS's `say`: Apple licenses them for personal, non-commercial use only.
set -euo pipefail
cd "$(dirname "$0")"
icon=../../../src-tauri/icons/Square284x284Logo.png
python="${FT_KOKORO_PYTHON:-python3}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Both recordings into $work/call.wav and $work/voice.wav.
if "$python" -c "import kokoro_onnx, numpy, soundfile" 2>/dev/null && "$python" make_voice.py "$work"; then
  echo "voices: Kokoro (af_heart, ef_dora)"
else
  echo "voices: espeak-ng (Kokoro is not available)" >&2
  espeak-ng -v en-us -s 145 -p 45 -w "$work/voice.wav" "Hi, this is FlickerTalk support. Your voice message arrived. This is an automatic reply so you can hear that voice messages work."
  espeak-ng -v en-us -s 145 -p 45 -w "$work/call.wav" "Hi, this is FlickerTalk's automated support contact. You've reached a recording, so your call works: audio and video are flowing end to end. Try sending a message, a file or a voice message, and they get answered. This call hangs up by itself in a couple of minutes. Thanks for trying FlickerTalk."
fi
# The same loudness as the manual's narration.
level="loudnorm=I=-16:TP=-1.5:LRA=11"

# The sample file sent back for any file: the app's icon.
cp "$icon" sample.png

# The voice message sent back for a voice message: AAC in MP4, what the app records itself, so
# it plays in the WebView of iOS and Android.
ffmpeg -loglevel error -y -i "$work/voice.wav" -af "$level" -ac 1 -ar 44100 -c:a aac -b:a 48k -movflags +faststart voice.m4a

# What a call hears, on a loop: Ogg Opus, 48 kHz mono, 20 ms packets, as the call sends them.
ffmpeg -loglevel error -y -i "$work/call.wav" -af "$level" -ac 1 -ar 48000 -c:a libopus -b:a 24k -frame_duration 20 -application voip call.opus

# What a video call sees: the icon gliding over a dark background. Four seconds that loop without
# a seam (the motion's period is the clip's length), H.264 Constrained Baseline in Annex-B,
# 640x480 at 15 fps, one slice per frame, an IDR with its SPS and PPS every second.
ffmpeg -loglevel error -y -f lavfi -i color=c=0x1d1b20:s=640x480:r=15:d=4 -loop 1 -framerate 15 -i "$icon" \
  -filter_complex "[1]scale=220:220[logo];[0][logo]overlay=x='(W-w)/2+170*sin(2*PI*t/4)':y='(H-h)/2':shortest=1,format=yuv420p" \
  -frames:v 60 -c:v libx264 -profile:v baseline -level 3.1 -preset slow -g 15 -keyint_min 15 -sc_threshold 0 -bf 0 \
  -x264-params "slices=1:repeat-headers=1:open-gop=0" -b:v 250k -maxrate 400k -bufsize 400k -f h264 call.h264

ls -l sample.png voice.m4a call.opus call.h264
