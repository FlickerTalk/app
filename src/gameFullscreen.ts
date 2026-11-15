import { nextTick, ref, watch } from "vue";
import { t } from "./i18n";

type Focusable = { focus: () => void };

// an Ionic button doesn't pass the focus on to the native button in its shadow root, which it draws a frame later
export async function focusButton(host: HTMLElement | null | undefined) {
  const ready = (host as { componentOnReady?: () => Promise<unknown> } | null | undefined)?.componentOnReady;
  if (ready) await ready.call(host);
  else if (host) await new Promise((drawn) => requestAnimationFrame(drawn));
  (host?.shadowRoot?.querySelector("button") ?? host)?.focus();
}

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
