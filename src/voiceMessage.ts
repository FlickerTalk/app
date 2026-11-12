import { computed, onUnmounted, ref, type Ref } from "vue";
import { sendFile } from "./core";
import { cancelRecording, recording, startRecording, stopRecording } from "./recorder";
import { t } from "./i18n";

export function useVoiceMessage(options: { chatId: () => string; error: Ref<string> }) {
  const now = ref(Date.now());
  let ticking: ReturnType<typeof setInterval> | undefined;
  const elapsed = computed(() => {
    const seconds = Math.max(0, Math.floor((now.value - recording.startedAt) / 1000));
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  });

  async function record() {
    options.error.value = "";
    const started = await startRecording();
    if (started !== "recording") {
      options.error.value = started === "unsupported" ? t("chat.cannotRecordHere") : t("chat.cannotRecord");
      return;
    }
    now.value = Date.now();
    ticking = setInterval(() => (now.value = Date.now()), 500);
  }

  async function sendVoice() {
    clearInterval(ticking);
    const voice = await stopRecording();
    if (voice) await sendFile(options.chatId(), voice);
  }

  function discardVoice() {
    clearInterval(ticking);
    cancelRecording();
  }

  onUnmounted(() => {
    if (recording.active) discardVoice();
  });

  return { recording, elapsed, record, sendVoice, discardVoice };
}
