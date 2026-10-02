/**
 * Ionic's overlays show their content only once the browser presents them, which happy-dom never
 * does. As Ionic's Vue testing guidance suggests, a modal is stubbed: its content while open, and a
 * way to dismiss it as Ionic would (`didDismiss`). Its attributes (a name, a class) stay on it.
 */
import { defineComponent, h } from "vue";

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
