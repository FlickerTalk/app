// The small picture of a video call (app#155): dragged anywhere, it settles into the nearest
// corner once let go, as calling apps do, never half over the other person's face.

/** A rectangle by its edges, as `getBoundingClientRect` gives them. */
export interface Edges {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * The corner of `area` nearest the centre of `box`, as where the box's top-left goes there.
 * `area` is where the whole picture may sit. A centre dead in the middle goes down and to the right.
 */
export function nearestCorner(box: Edges, area: Edges): { x: number; y: number } {
  const width = box.right - box.left;
  const height = box.bottom - box.top;
  const left = box.left + box.right < area.left + area.right;
  const top = box.top + box.bottom < area.top + area.bottom;
  return { x: left ? area.left : area.right - width, y: top ? area.top : area.bottom - height };
}

/**
 * Where the small picture may settle on `screen`: as far from either side as it starts from its
 * own (`home`, the picture undragged), from that far under `top` (the buttons at the top of the
 * screen) down to where its bottom starts, over the call's buttons.
 */
export function thumbArea(screen: Edges, home: Edges, top: number): Edges {
  const margin = Math.min(home.left - screen.left, screen.right - home.right);
  return { left: screen.left + margin, top: top + margin, right: screen.right - margin, bottom: home.bottom };
}
