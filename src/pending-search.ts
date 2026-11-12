let asked: string | null = null;

export function askSearch(id: string): void {
  asked = id;
}

export function takeSearch(id: string): boolean {
  if (asked !== id) return false;
  asked = null;
  return true;
}

let pane = "";

export function showInPane(id: string): void {
  pane = id;
}

export function inPane(id: string): boolean {
  return pane !== "" && pane === id;
}
