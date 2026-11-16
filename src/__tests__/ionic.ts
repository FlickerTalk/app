import { defineComponent, h, onMounted, ref, watch } from "vue";

export const IonModalStub = defineComponent({
  name: "IonModal",
  props: {
    isOpen: Boolean,
    breakpoints: { type: Array, default: undefined },
    initialBreakpoint: { type: Number, default: undefined },
  },
  emits: ["didDismiss"],
  setup(props, { slots }) {
    return () => (props.isOpen ? h("div", { "data-stub": "ion-modal" }, slots.default?.()) : null);
  },
});

export interface LateIonModal {
  present: () => void;
  finishPresenting: () => void;
}

export const LateIonModalStub = defineComponent({
  name: "IonModal",
  props: {
    isOpen: Boolean,
    breakpoints: { type: Array, default: undefined },
    initialBreakpoint: { type: Number, default: undefined },
  },
  emits: ["willPresent", "didDismiss"],
  setup(props, { slots, emit, expose }) {
    const host = ref<HTMLElement>();
    const shown = ref(false);
    let loaded = false;
    let presenting = false;
    let dismissAfter = false;

    function close() {
      if (!shown.value) return false;
      shown.value = false;
      emit("didDismiss");
      return true;
    }

    async function dismiss() {
      if (!presenting) return close();
      dismissAfter = true;
      return true;
    }

    function present() {
      loaded = true;
      presenting = true;
      shown.value = true;
      emit("willPresent");
    }

    function finishPresenting() {
      presenting = false;
      if (!dismissAfter) return;
      dismissAfter = false;
      close();
    }

    watch(
      () => props.isOpen,
      (open) => {
        if (!open) void dismiss();
        else if (loaded) {
          present();
          finishPresenting();
        }
      },
    );
    onMounted(() => Object.assign(host.value!, { dismiss }));
    expose({ present, finishPresenting } satisfies LateIonModal);
    return () => h("div", { ref: host, "data-stub": "ion-modal" }, shown.value ? slots.default?.() : undefined);
  },
});
