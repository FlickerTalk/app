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
