import { onBeforeUnmount, onMounted, watch, type WatchSource } from "vue";

export function followLook(look: () => Record<string, unknown>, tell: (message: Record<string, unknown>) => void, state: WatchSource) {
  let told = "";
  const send = () => {
    const now = look();
    const said = JSON.stringify(now);
    if (said === told) return;
    told = said;
    tell({ type: "ft.theme", ...now });
  };
  const page = new MutationObserver(send);
  watch(state, send);
  onMounted(() => {
    told = JSON.stringify(look());
    page.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-direction", "dir"] });
  });
  onBeforeUnmount(() => page.disconnect());
}
