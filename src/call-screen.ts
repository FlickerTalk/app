// The call screen on the page (2026-10-09). The route leaves it at the start of its transition,
// but Ionic takes it away only at the end: a push back to the call in between found that leaving
// screen, and its removal left a blank page. Whoever goes back to the call waits for it to be gone.
import { ref, watch } from "vue";

/** Call screens mounted: the one on screen, and one still leaving. */
const mounted = ref(0);

/** A call screen is on the page; the function returned says it unmounted (once). */
export function callScreenMounted(): () => void {
  mounted.value++;
  let done = false;
  return () => {
    if (done) return;
    done = true;
    mounted.value--;
  };
}

/** Resolves once no call screen is on the page: at once, or when the one leaving has gone. */
export function callScreenGone(): Promise<void> {
  if (!mounted.value) return Promise.resolve();
  return new Promise((resolve) => {
    const stop = watch(
      mounted,
      (count) => {
        if (count) return;
        stop();
        resolve();
      },
      { flush: "sync" },
    );
  });
}
