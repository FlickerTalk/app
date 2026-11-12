import type { PickedFile } from "./core";

// the proposal only fills the composer, the user sends it, never the tool

export type Proposal = { kind: "text"; text: string } | { kind: "file"; file: PickedFile };

let offered: { contact: string; proposal: Proposal } | null = null;

export function offerSend(contact: string, proposal: Proposal): void {
  offered = contact ? { contact, proposal } : null;
}

export function takeSend(contact: string): Proposal | null {
  if (!offered || offered.contact !== contact) return null;
  const { proposal } = offered;
  offered = null;
  return proposal;
}

export interface ContactAsk {
  answer: (contact: string | null) => void;
}
