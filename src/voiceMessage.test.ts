import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";
import { calls, seed } from "./__tests__/seed";

const recorder = vi.hoisted(() => ({
  startRecording: vi.fn(async (): Promise<string> => "recording"),
  stopRecording: vi.fn(async (): Promise<File | null> => new File(["voice"], "voice.m4a", { type: "audio/mp4" })),
  cancelRecording: vi.fn(),
}));
vi.mock("./recorder", async () => {
  const { reactive } = await import("vue");
  const recording = reactive({ active: false, startedAt: 0 });
  recorder.startRecording.mockImplementation(async () => {
    recording.active = true;
    recording.startedAt = Date.now();
    return "recording";
  });
  recorder.stopRecording.mockImplementation(async () => {
    recording.active = false;
    return new File(["voice"], "voice.m4a", { type: "audio/mp4" });
  });
  recorder.cancelRecording.mockImplementation(() => {
    recording.active = false;
  });
  return { recording, ...recorder };
});

import { useVoiceMessage } from "./voiceMessage";

function host() {
  const error = ref("");
  let voice!: ReturnType<typeof useVoiceMessage>;
  const wrapper = mount(
    defineComponent({
      setup() {
        voice = useVoiceMessage({ chatId: () => "c1", error });
        return () => h("div");
      },
    }),
  );
  return { voice, error, wrapper };
}

describe("useVoiceMessage", () => {
  beforeEach(() => {
    seed();
    recorder.startRecording.mockClear();
    recorder.cancelRecording.mockClear();
  });
  afterEach(() => vi.useRealTimers());

  it("counts the minutes and seconds of the recording", async () => {
    vi.useFakeTimers();
    const { voice, wrapper } = host();
    await voice.record();
    expect(voice.recording.active).toBe(true);
    expect(voice.elapsed.value).toBe("0:00");
    vi.advanceTimersByTime(65_000);
    expect(voice.elapsed.value).toBe("1:05");
    voice.discardVoice();
    wrapper.unmount();
  });

  it("sends the recording as a file of the conversation", async () => {
    const { voice } = host();
    await voice.record();
    await voice.sendVoice();
    await flushPromises();
    expect(calls).toContainEqual(["core_send_file", { contact: "c1", upload: "up1", name: "voice.m4a", mime: "audio/mp4" }]);
    expect(voice.recording.active).toBe(false);
  });

  it("says why it cannot record", async () => {
    const { voice, error } = host();
    recorder.startRecording.mockImplementationOnce(async () => "unsupported");
    await voice.record();
    expect(error.value).toContain("Can't record here");
    recorder.startRecording.mockImplementationOnce(async () => "denied");
    await voice.record();
    expect(error.value).toContain("microphone");
    expect(voice.recording.active).toBe(false);
  });

  it("throws away a recording still going when the conversation goes", async () => {
    const { voice, wrapper } = host();
    await voice.record();
    wrapper.unmount();
    expect(recorder.cancelRecording).toHaveBeenCalledTimes(1);
    expect(calls.some(([command]) => command === "core_send_file")).toBe(false);
  });
});
