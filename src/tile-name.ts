import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";

export const NAME_MIN_PX = 9;

export interface NameFit {
  size?: number;
  clipped: boolean;
}

const tenthBelow = (value: number) => Math.floor(value * 10 + 1e-9) / 10;

export function fitName(widthAt: (size: number) => number, available: number, base: number): NameFit {
  if (available <= 0) return { clipped: false };
  let size = base;
  let width = widthAt(size);
  if (width <= available) return { clipped: false };
  while (size > NAME_MIN_PX) {
    size = Math.max(NAME_MIN_PX, Math.min(tenthBelow(size - 0.1), tenthBelow((size * available) / width)));
    width = widthAt(size);
    if (width <= available) return { size, clipped: false };
  }
  return { size: NAME_MIN_PX, clipped: true };
}

export function useNameFit(box: Ref<HTMLElement | undefined>, probe: Ref<HTMLElement | undefined>): Readonly<Ref<NameFit>> {
  const fit = ref<NameFit>({ clipped: false });
  let observer: ResizeObserver | undefined;

  function measure() {
    const room = box.value;
    const words = probe.value;
    if (!room || !words) return;
    const base = parseFloat(getComputedStyle(words).fontSize);
    const widthAt = (size: number) => {
      words.style.fontSize = `${size}px`;
      return words.getBoundingClientRect().width;
    };
    fit.value = fitName(widthAt, room.getBoundingClientRect().width, base);
    words.style.fontSize = "";
  }

  onMounted(() => {
    if (typeof ResizeObserver === "undefined") return;
    observer = new ResizeObserver(measure);
    for (const element of [box.value, probe.value]) if (element) observer.observe(element);
  });
  onBeforeUnmount(() => observer?.disconnect());

  return fit;
}
