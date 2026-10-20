import { computed, inject, provide, ref, type InjectionKey, type Ref } from "vue";
import { onIonViewDidEnter, onIonViewWillEnter, onIonViewWillLeave } from "@ionic/vue";

/** Whether the page holding this one is on screen: the tabs page, for a tab. */
const HOLDER_ON_SCREEN: InjectionKey<Readonly<Ref<boolean>>> = Symbol("holderOnScreen");

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
 *
 * A page inside another one (a tab inside the tabs page) is on screen only while that one is too
 * (app#197): a page pushed over the tabs is Ionic's word to the tabs page alone, never to the tab
 * inside it. So each page tells the pages inside it whether it is on screen.
 */
export function useOnScreen(entered?: () => void): Readonly<Ref<boolean>> {
  const holder = inject(HOLDER_ON_SCREEN, undefined);
  const own = ref(true);
  let entering = false;
  onIonViewWillEnter(() => (entering = true));
  onIonViewDidEnter(() => {
    if (!entering) return;
    own.value = true;
    entered?.();
  });
  onIonViewWillLeave(() => {
    entering = false;
    own.value = false;
  });
  const onScreen = computed(() => own.value && (holder?.value ?? true));
  provide(HOLDER_ON_SCREEN, onScreen);
  return onScreen;
}
