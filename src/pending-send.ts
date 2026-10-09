import type { PickedFile } from "./core";

// Ioan, 2026-10-09: a tool opened on its own (from Apps, from a reminder) has no chat behind it, so
// what it proposes to send asks the user who it is for. The proposal waits here for the conversation
// picked, which puts it in its composer once it is on screen: the user sends it, never the tool
// (§53). Only the latest proposal counts, and it is taken once.

/** What a tool proposes: a text for the composer, or a file staged as an attachment. */
export type Proposal = { kind: "text"; text: string } | { kind: "file"; file: PickedFile };

let offered: { contact: string; proposal: Proposal } | null = null;

/** The conversation with `contact` is to show `proposal` in its composer when it is next on screen. */
export function offerSend(contact: string, proposal: Proposal): void {
  offered = contact ? { contact, proposal } : null;
}

/** What waits for the conversation with `contact`, if anything; it goes with the answer. */
export function takeSend(contact: string): Proposal | null {
  if (!offered || offered.contact !== contact) return null;
  const { proposal } = offered;
  offered = null;
  return proposal;
}
