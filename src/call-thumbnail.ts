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
