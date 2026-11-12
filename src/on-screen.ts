import { computed, inject, provide, ref, type InjectionKey, type Ref } from "vue";
import { onIonViewDidEnter, onIonViewWillEnter, onIonViewWillLeave } from "@ionic/vue";

const HOLDER_ON_SCREEN: InjectionKey<Readonly<Ref<boolean>>> = Symbol("holderOnScreen");

// Ionic fires "will leave" as the next page starts, but "did enter" only after its own transition,
// so a late "did enter" counts only if no "will leave" came after "will enter".
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
