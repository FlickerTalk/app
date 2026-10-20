import { ref, type Ref } from "vue";
import { onIonViewDidEnter, onIonViewWillEnter, onIonViewWillLeave } from "@ionic/vue";

/**
 * Whether this Ionic page is the one on screen; call it from the page's `setup`. Ionic keeps a page
 * mounted under the next one, so what the page leaves open (a sheet, a plugin, the read receipts)
 * must know when it is seen.
 *
 * Ionic says a page leaves as soon as the next one starts to come in, but that it came in only once
 * its own transition is over (app#179, app#193). With a page opened over this one while it still
 * came in, that late word would put it back on screen under the page on top. So a "did enter"
 * counts only if no "will leave" came after the page's "will enter"; `entered` runs on each one
 * that counts.
 */
export function useOnScreen(entered?: () => void): Ref<boolean> {
  const onScreen = ref(true);
  let entering = false;
  onIonViewWillEnter(() => (entering = true));
  onIonViewDidEnter(() => {
    if (!entering) return;
    onScreen.value = true;
    entered?.();
  });
  onIonViewWillLeave(() => {
    entering = false;
    onScreen.value = false;
  });
  return onScreen;
}
