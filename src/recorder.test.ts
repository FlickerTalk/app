import { beforeEach, describe, expect, it, vi } from "vitest";
import * as recorder from "./recorder";

class FakeTrack {
  stopped = false;
  stop() {
    this.stopped = true;
  }
}

class FakeRecorder {
  static last: FakeRecorder;
  state = "inactive";
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  constructor(
    public stream: unknown,
    public options: { mimeType: string },
  ) {
    FakeRecorder.last = this;
  }
  start() {
    this.state = "recording";
  }
  stop() {
    this.state = "inactive";
    this.ondataavailable?.({ data: new Blob(["voice"], { type: this.options.mimeType }) });
    this.onstop?.();
  }
}

let track: FakeTrack;

// Voice messages (decision 2026-09-22): recorded in the WebView, sent as an audio file, P2P.
describe("recorder", () => {
  beforeEach(() => {
    track = new FakeTrack();
    recorder.media.getUserMedia = vi.fn(async () => ({ getTracks: () => [track] }) as unknown as MediaStream);
    recorder.media.createRecorder = (stream, options) => new FakeRecorder(stream, options) as unknown as MediaRecorder;
    recorder.media.isTypeSupported = (type) => type.startsWith("audio/mp4");
    recorder.cancelRecording();
  });

  // AAC in MP4 plays on Android and iOS alike; iOS records nothing else.
  it("prefers AAC in MP4, then Opus in WebM", () => {
    expect(recorder.pickFormat((type) => type.startsWith("audio/mp4"))).toEqual({ mime: "audio/mp4", extension: "m4a" });
    expect(recorder.pickFormat((type) => type.startsWith("audio/webm"))).toEqual({ mime: "audio/webm", extension: "webm" });
    expect(recorder.pickFormat(() => false)).toBeNull();
  });

  it("records the microphone and hands back a voice file", async () => {
    expect(await recorder.startRecording()).toBe("recording");
    expect(recorder.media.getUserMedia).toHaveBeenCalledWith({ audio: true });
    expect(recorder.recording.active).toBe(true);
    expect(FakeRecorder.last.options.mimeType).toBe("audio/mp4;codecs=mp4a.40.2");

    const file = await recorder.stopRecording();
    expect(file?.name).toMatch(/^voice-\d{8}-\d{6}\.m4a$/);
    expect(file?.type).toBe("audio/mp4");
    expect(file?.size).toBe(5);
    expect(recorder.recording.active).toBe(false);
    expect(track.stopped).toBe(true);
  });

  it("throws the recording away when cancelled", async () => {
    await recorder.startRecording();
    recorder.cancelRecording();
    expect(recorder.recording.active).toBe(false);
    expect(track.stopped).toBe(true);
    expect(await recorder.stopRecording()).toBeNull();
  });

  // Why it cannot record, so the user (and we) know what to fix.
  it("says when the microphone is not allowed", async () => {
    recorder.media.getUserMedia = vi.fn(async () => {
      throw new Error("denied");
    });
    expect(await recorder.startRecording()).toBe("denied");
    expect(recorder.recording.active).toBe(false);
  });

  it("says when this WebView has no recorder for a playable format", async () => {
    recorder.media.isTypeSupported = () => false;
    expect(await recorder.startRecording()).toBe("unsupported");
  });
});
