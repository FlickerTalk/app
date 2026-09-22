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
  store.me = { ...fixture.me, mailbox: true, freeUntil: 0 };
  store.chats = fixture.chats.map(
    (chat): Chat => ({
      ...chat,
      status: chat.status as Status,
      blocked: false,
      messages: chat.messages.map(
        (message): ChatMessage => ({
          ...message,
          text: message.text ?? "",
          status: message.status as Status,
          kind: message.kind === "file" ? "file" : undefined,
          file: message.file && { ...message.file, mime: "", state: message.file.state as FileState },
        }),
      ),
    }),
  );
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
      }));
    }
    if (command === "core_conversations") return [];
    if (command === "core_card") return "https://flickertalk.com/add#card";
    if (command === "core_upload_start") return "up1";
    if (command === "core_contact") {
      return { id: args?.contact, name: "Maria López", fingerprint: "a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90 a1b2 c3d4 e5f6 0718", mailbox: true, blocked: false };
    }
    return undefined;
  });
}
