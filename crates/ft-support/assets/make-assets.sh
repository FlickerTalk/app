#!/usr/bin/env bash
# Rebuilds the media the support peer carries inside its binary (`src/clips.rs`), from the texts
# below and the app's icon. Run it from anywhere; it needs espeak-ng and an ffmpeg with libx264
# and libopus. The voices are espeak-ng's own (no MBROLA voices: their licences forbid commercial
# use). Commit the four files it writes.
set -euo pipefail
cd "$(dirname "$0")"
icon=../../../src-tauri/icons/Square284x284Logo.png
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

say() { # say <wav> <text>
  espeak-ng -v en-us -s 145 -p 45 -w "$1" "$2"
}

# The sample file sent back for any file: the app's icon.
cp "$icon" sample.png

# The voice message sent back for a voice message: AAC in MP4, what the app records itself, so
# it plays in the WebView of iOS and Android.
say "$work/voice.wav" "Hi! This is the FlickerTalk support contact. Your voice message arrived, end to end encrypted. This answer is automatic: a program sends it, not a person."
ffmpeg -loglevel error -y -i "$work/voice.wav" -ac 1 -ar 44100 -c:a aac -b:a 48k -movflags +faststart voice.m4a

# What a call hears, on a loop: Ogg Opus, 48 kHz mono, 20 ms packets, as the call sends them.
say "$work/call.wav" "Hello, and thanks for calling. This is the FlickerTalk automated support line. Your call is end to end encrypted, and nothing is recorded. I cannot hear you: this message repeats until you hang up, and the call ends by itself after two minutes. For help from a person, visit flickertalk dot com slash support."
ffmpeg -loglevel error -y -i "$work/call.wav" -ac 1 -ar 48000 -c:a libopus -b:a 24k -frame_duration 20 -application voip call.opus

# What a video call sees: the icon gliding over a dark background. Four seconds that loop without
# a seam (the motion's period is the clip's length), H.264 Constrained Baseline in Annex-B,
# 640x480 at 15 fps, one slice per frame, an IDR with its SPS and PPS every second.
ffmpeg -loglevel error -y -f lavfi -i color=c=0x1d1b20:s=640x480:r=15:d=4 -loop 1 -framerate 15 -i "$icon" \
  -filter_complex "[1]scale=220:220[logo];[0][logo]overlay=x='(W-w)/2+170*sin(2*PI*t/4)':y='(H-h)/2':shortest=1,format=yuv420p" \
  -frames:v 60 -c:v libx264 -profile:v baseline -level 3.1 -preset slow -g 15 -keyint_min 15 -sc_threshold 0 -bf 0 \
  -x264-params "slices=1:repeat-headers=1:open-gop=0" -b:v 250k -maxrate 400k -bufsize 400k -f h264 call.h264

ls -l sample.png voice.m4a call.opus call.h264
