import { computed, reactive, ref, watch, type Ref } from "vue";

export interface Edges {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function nearestCorner(box: Edges, area: Edges): { x: number; y: number } {
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  const left = box.left + box.right < area.left + area.right;
  const top = box.top + box.bottom < area.top + area.bottom;
  return { x: left ? area.left : area.right - width, y: top ? area.top : area.bottom - height };
}

export function thumbArea(screen: Edges, home: Edges, top: number): Edges {
  const margin = Math.min(home.left - screen.left, screen.right - home.right);
  return { left: screen.left + margin, top: top + margin, right: screen.right - margin, bottom: home.bottom };
}

export interface ThumbDragOptions {
  body: Readonly<Ref<HTMLElement | null>>;
  remoteSlot: Readonly<Ref<HTMLElement | null>>;
  localSlot: Readonly<Ref<HTMLElement | null>>;
  remoteThumb: Readonly<Ref<boolean>>;
  localThumb: Readonly<Ref<boolean>>;
  above: () => Array<{ y: number; height: number } | null>;
  moved: () => void;
}

const SETTLE_MS = 200;

export function useThumbDrag({ body, remoteSlot, localSlot, remoteThumb, localThumb, above, moved }: ThumbDragOptions) {
  const drag = reactive({ x: 0, y: 0 });
  let grab: { id: number; x: number; y: number; fromX: number; fromY: number; slot: Element } | null = null;
  const dragging = ref(false);
  let settling = 0;
  const thumbStyle = computed(() => ((localThumb.value || remoteThumb.value) && (drag.x || drag.y) ? { transform: `translate(${drag.x}px, ${drag.y}px)` } : undefined));
  watch([localThumb, remoteThumb], () => {
    endDrag();
    Object.assign(drag, { x: 0, y: 0 });
  });
  watch(thumbStyle, moved, { flush: "post" });
  const thumbSlot = () => (remoteThumb.value ? remoteSlot.value : localThumb.value ? localSlot.value : null);
  function grabThumb(event: PointerEvent) {
    if (!thumbSlot() || event.currentTarget !== thumbSlot()) return;
    const slot = event.currentTarget as Element;
    endDrag();
    grab = { id: event.pointerId, x: event.clientX, y: event.clientY, fromX: drag.x, fromY: drag.y, slot };
    dragging.value = true;
    slot.setPointerCapture?.(event.pointerId);
    // on the iPhone the slot stopped getting pointer events after a few points
    window.addEventListener("pointermove", moveThumb);
    window.addEventListener("pointerup", dropThumb);
    window.addEventListener("pointercancel", dropThumb);
  }
  function endDrag() {
    cancelAnimationFrame(settling);
    settling = 0;
    window.removeEventListener("pointermove", moveThumb);
    window.removeEventListener("pointerup", dropThumb);
    window.removeEventListener("pointercancel", dropThumb);
    dragging.value = false;
    if (!grab) return;
    const { slot, id } = grab;
    grab = null;
    try {
      slot.releasePointerCapture?.(id);
    } catch {
      // pointer already gone after a cancelled touch
    }
  }
  function moveThumb(event: PointerEvent) {
    if (!grab || event.pointerId !== grab.id) return;
    drag.x = grab.fromX + event.clientX - grab.x;
    drag.y = grab.fromY + event.clientY - grab.y;
    moved();
  }
  function dropThumb(event: PointerEvent) {
    if (!grab || event.pointerId !== grab.id) return;
    endDrag();
    const box = thumbSlot()?.getBoundingClientRect();
    const screen = body.value?.getBoundingClientRect();
    if (box && screen && screen.width > 0 && screen.height > 0) {
      const home = { left: box.left - drag.x, top: box.top - drag.y, right: box.right - drag.x, bottom: box.bottom - drag.y };
      const tops = above().map((rect) => (rect ? rect.y + rect.height : screen.top));
      const corner = nearestCorner(box, thumbArea(screen, home, Math.max(screen.top, ...tops)));
      settle({ x: Math.round(drag.x + corner.x - box.left), y: Math.round(drag.y + corner.y - box.top) });
    }
    moved();
  }

  // one step per frame, the native view can't follow a CSS transition
  function settle(to: { x: number; y: number }) {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      Object.assign(drag, to);
      return;
    }
    const from = { x: drag.x, y: drag.y };
    let start: number | undefined;
    const step = (now: number) => {
      start ??= now;
      const done = Math.min(1, (now - start) / SETTLE_MS);
      const eased = 1 - (1 - done) ** 3;
      Object.assign(drag, done < 1 ? { x: from.x + (to.x - from.x) * eased, y: from.y + (to.y - from.y) * eased } : to);
      settling = done < 1 ? requestAnimationFrame(step) : 0;
    };
    settling = requestAnimationFrame(step);
  }

  return { dragging, thumbStyle, grabThumb, moveThumb, dropThumb, endDrag };
}
