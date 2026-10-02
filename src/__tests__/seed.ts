/**
 * Fills the real store with the fixture conversations and makes the core bridge answer from them,
 * so view tests show realistic data without a Rust core.
 */
import fixture from "./chats.fixture.json";
import { store, type Chat, type ChatMessage, type FileState, type Status } from "../core";
import { installTauri } from "./tauri";

export { fixture };

/** Every command sent to the core since the last `seed()`. */
export const calls: Array<[string, Record<string, unknown> | undefined]> = [];

const at = (time: string) => {
  const [hours, minutes] = time.split(":").map(Number);
  return new Date(2026, 8, 22, hours, minutes).getTime();
};

export function seed(): void {
  store.me = { ...fixture.me, mailbox: true, receipts: true, freeUntil: 0, autoDownload: 10 * 1024 * 1024 };
  store.chats = fixture.chats.map(
    (chat): Chat => ({
      ...chat,
      status: chat.status as Status,
      lastKind: "text",
      blocked: false,
      messages: chat.messages.map(
        (message): ChatMessage => ({
          ...message,
          text: message.text ?? "",
          sentAt: at(message.time),
          status: message.status as Status,
          kind: message.kind === "file" ? "file" : undefined,
          file: message.file && { ...message.file, mime: (message.file as { mime?: string }).mime ?? "", state: message.file.state as FileState },
        }),
      ),
    }),
  );
  store.sessions = [];
  store.requests = [];
  store.ready = true;
  calls.length = 0;
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_messages") {
      const chat = fixture.chats.find((candidate) => candidate.id === args?.contact);
      return (chat?.messages ?? []).map((message) => ({
        id: message.id,
        outgoing: message.mine,
        text: message.text ?? "",
        sentAt: at(message.time),
        state: message.status ?? "delivered",
        // A file as the core would describe it, so a tap on it finds its kind.
        ...(message.file
          ? { file: { name: message.file.name, size: 1_200_000, mime: (message.file as { mime?: string }).mime ?? "application/octet-stream", state: message.file.state, progress: message.file.progress, path: `/data/files/${message.file.name}` } }
          : {}),
      }));
    }
    if (command === "core_conversations") return [];
    if (command === "core_circle_messages") return (store.circles.find((one) => one.id === args?.circle)?.messages ?? []).map((m) => ({ id: m.id, outgoing: m.mine, sender: m.sender, senderName: m.senderName, kind: m.kind, text: m.text, sentAt: at(m.time), state: m.status ?? "delivered" }));
    if (command === "core_card") return "https://flickertalk.com/add#card";
    // A hidden session: the same PIN gives the same session; here, always an empty one.
    if (command === "core_session_open") return { id: "s1", conversations: [], requests: [] };
    if (command === "core_session_create") return { id: "s2", conversations: [], requests: [] };
    if (command === "core_requests") return [];
    if (command === "core_plugin_made") return { sent: false, staged: { path: "/data/files/outgoing/1-clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 } };
    if (command === "core_upload_start") return "up1";
    if (command === "core_take_photo") return [{ path: "/data/uploads/photo.jpg", name: "photo-20260923-201530.jpg", mime: "image/jpeg", size: 1234 }];
    if (command === "core_contact") {
      return { id: args?.contact, name: "Maria López", fingerprint: "a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90 a1b2 c3d4 e5f6 0718", mailbox: true, blocked: false, keepFor: 0, burnAfterRead: 0, rules: { muted: false, acceptsChat: true, acceptsCalls: true, receipts: true } };
    }
    // One plugin, allowed to read what the user hands it (issue app#3).
    if (command === "core_plugins") {
      return [
        {
          id: "com.flickertalk.code",
          name: "Code block",
          version: "1.0.0",
          asks: { network: [], messages: true, send: "nothing" },
          granted: { network: [], messages: true, send: "nothing" },
          installedAt: at("09:00"),
        },
      ];
    }
    return undefined;
  });
}
