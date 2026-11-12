import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { effectScope, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import { chat, type ChatMessage } from "./core";
import { useMessageActions } from "./messageActions";

function actionsIn(chatId = "c1") {
  const draft = ref("");
  const editing = ref<ChatMessage | null>(null);
  const replying = ref<ChatMessage | null>(null);
  const scope = effectScope();
  const actions = scope.run(() => useMessageActions({ chatId: () => chatId, messages: () => chat(chatId)?.messages ?? [], draft, editing, replying }))!;
  return { actions, draft, editing, replying, stop: () => scope.stop() };
}

describe("useMessageActions", () => {
  beforeEach(() => seed());

  it("opens on a message with the first set of actions and closes leaving nothing chosen", () => {
    const { actions, stop } = actionsIn();
    actions.forwarding.value = true;
    actions.act("m1");
    expect(actions.acting.value).toBe("m1");
    expect(actions.actingMessage.value?.text).toBe("Are you coming tonight?");
    expect([actions.forwarding.value, actions.erasing.value, actions.openingWith.value]).toEqual([false, false, false]);
    actions.erasing.value = true;
    actions.openingWith.value = true;
    actions.closeActions();
    expect(actions.acting.value).toBe("");
    expect([actions.forwarding.value, actions.erasing.value, actions.openingWith.value]).toEqual([false, false, false]);
    stop();
  });

  it("folds a message and unfolds it the next time", () => {
    const { actions, stop } = actionsIn();
    actions.act("m1");
    actions.fold();
    expect(actions.folded.has("m1")).toBe(true);
    expect(actions.acting.value).toBe("");
    actions.act("m1");
    actions.fold();
    expect(actions.folded.has("m1")).toBe(false);
    stop();
  });

  it("reacts with an emoji, and takes the same one back", async () => {
    const { actions, stop } = actionsIn();
    actions.act("m1");
    await actions.reactWith("👍");
    expect(calls).toContainEqual(["core_react", { contact: "c1", message: "m1", emoji: "👍" }]);
    chat("c1")!.messages[0].reactions = { mine: "👍" };
    actions.act("m1");
    await actions.reactWith("👍");
    expect(calls).toContainEqual(["core_react", { contact: "c1", message: "m1" }]);
    stop();
  });

  it("pins a message and unpins a pinned one", async () => {
    const { actions, stop } = actionsIn();
    actions.act("m1");
    await actions.togglePin();
    chat("c1")!.messages[0].pinned = true;
    actions.act("m1");
    await actions.togglePin();
    expect(calls.filter(([command]) => command === "core_pin")).toEqual([
      ["core_pin", { message: "m1", pinned: true }],
      ["core_pin", { message: "m1", pinned: false }],
    ]);
    stop();
  });

  it("answers a message, or edits one of mine with its words in the composer", () => {
    const { actions, draft, editing, replying, stop } = actionsIn();
    actions.act("m1");
    actions.reply();
    expect(replying.value?.id).toBe("m1");
    expect(actions.acting.value).toBe("");

    actions.act("m2");
    expect(actions.canEdit.value).toBe(true);
    actions.edit();
    expect(replying.value).toBeNull();
    expect(editing.value?.id).toBe("m2");
    expect(draft.value).toBe("Yes! Leaving work at 5:30");
    stop();
  });

  it("edits neither their messages, nor my files, nor what was not sent", () => {
    const { actions, stop } = actionsIn();
    actions.act("m1");
    expect(actions.canEdit.value).toBe(false);
    actions.act("m4");
    expect(actions.canEdit.value).toBe(false);
    chat("c1")!.messages[1].status = "unsent";
    actions.act("m2");
    expect(actions.canEdit.value).toBe(false);
    stop();
  });

  it("erases here and reads the conversation again, or takes it back for both sides", async () => {
    const { actions, stop } = actionsIn();
    actions.act("m1");
    actions.fold();
    actions.act("m1");
    await actions.erase();
    expect(actions.folded.has("m1")).toBe(false);
    expect(calls).toContainEqual(["core_forget_message", { message: "m1" }]);
    expect(calls).toContainEqual(["core_messages", { contact: "c1", limit: 200 }]);
    actions.act("m2");
    await actions.eraseEverywhere();
    expect(calls).toContainEqual(["core_delete_everyone", { message: "m2" }]);
    stop();
  });

  it("forwards to anyone but the contact of this conversation, and shares with another app", async () => {
    const { actions, stop } = actionsIn();
    expect(actions.others.value.map((one) => one.id)).not.toContain("c1");
    expect(actions.others.value.length).toBeGreaterThan(0);
    actions.act("m1");
    await actions.forwardTo("c2");
    expect(calls).toContainEqual(["core_forward", { message: "m1", contact: "c2" }]);
    actions.act("m3");
    await actions.share();
    await flushPromises();
    expect(calls).toContainEqual(["core_share_message", { message: "m3" }]);
    expect(actions.acting.value).toBe("");
    stop();
  });
});
