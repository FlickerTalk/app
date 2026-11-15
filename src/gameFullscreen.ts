import { nextTick, ref, watch } from "vue";
import { t } from "./i18n";

type Focusable = { focus: () => void };

export function useFullscreenCues(options: {
  on: () => boolean;
  inside: () => Focusable | null | undefined;
  outside: () => Focusable | null | undefined;
  quiet?: () => boolean;
}) {
  const said = ref("");
  // the same words twice are only read again if the region was emptied in between
  function announce(text: string) {
    said.value = "";
    void nextTick(() => (said.value = text));
  }
  watch(options.on, async (now) => {
    if (!now && options.quiet?.()) return;
    await nextTick();
    (now ? options.inside() : options.outside())?.focus();
    announce(t(now ? "games.enterFullscreen" : "games.exitFullscreen"));
  });
  return { said, announce };
}
