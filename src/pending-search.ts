// Ioan, 2026-10-06: as in WhatsApp, the contact page's search goes back to the conversation it
// came from, and the conversation opens its search once it is on screen again. Only the latest
// request counts, and it is taken once.
let asked: string | null = null;

/** The conversation `id` is to open its search when it is next on screen. */
export function askSearch(id: string): void {
  asked = id;
}

/** Whether the conversation `id` was asked to search; the request goes with the answer. */
export function takeSearch(id: string): boolean {
  if (asked !== id) return false;
  asked = null;
  return true;
}

// QA of 1.4.0 (2026-10-06): on a wide screen the conversation is in the chats tab's pane, beside
// the list, not on a page of its own. The chats tab says which one; "" is none.
let pane = "";

/** The conversation the chats tab shows beside its list, or "" when it shows none. */
export function showInPane(id: string): void {
  pane = id;
}

/** Whether the chats tab shows the conversation `id` beside its list. */
export function inPane(id: string): boolean {
  return pane !== "" && pane === id;
}
