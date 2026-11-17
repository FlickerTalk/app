import { describe, expect, it } from "vitest";

import { fieldFromFrame, fromFrame, NOTICE_LIMIT } from "./frame-messages";

describe("where a plugin's frame says its focused field is", () => {
  const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
  const said = (data: unknown, source: unknown = frame.contentWindow) => fieldFromFrame({ source, data } as unknown as MessageEvent, frame);

  it("takes a field's top and bottom from the plugin's own frame only", () => {
    expect(said({ type: "ft.field", top: 206, bottom: 254 })).toEqual({ top: 206, bottom: 254 });
    expect(said({ type: "ft.field", top: 206, bottom: 254 }, {})).toBeNull();
    expect(fieldFromFrame({ source: null, data: { type: "ft.field", top: 1, bottom: 2 } } as unknown as MessageEvent, null)).toBeNull();
  });

  it("refuses anything else, and a field upside down or out of any screen", () => {
    expect(said({ type: "ft.height", height: 120 })).toBeNull();
    expect(said({ type: "ft.field", top: "206", bottom: 254 })).toBeNull();
    expect(said({ type: "ft.field", top: 254, bottom: 206 })).toBeNull();
    expect(said({ type: "ft.field", top: Number.NaN, bottom: 254 })).toBeNull();
    expect(said({ type: "ft.field", top: 0, bottom: Number.POSITIVE_INFINITY })).toBeNull();
    expect(said({ type: "ft.field", top: -1e6, bottom: 20 })).toBeNull();
    expect(said(null)).toBeNull();
  });

  it("is not something the plugin can ask the app for", () => {
    expect(fromFrame({ source: frame.contentWindow, data: { type: "ft.field", top: 1, bottom: 2 } } as unknown as MessageEvent, frame)).toBeNull();
  });
});

describe("what a plugin's frame may say", () => {
  it("listens only to the frame of the plugin", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const good = { source: frame.contentWindow, data: { type: "ft.height", height: 120 } } as unknown as MessageEvent;
    expect(fromFrame(good, frame)).toEqual({ type: "ft.height", height: 120 });

    const elsewhere = { source: {}, data: { type: "ft.height", height: 1 } } as unknown as MessageEvent;
    expect(fromFrame(elsewhere, frame)).toBeNull();

    const nonsense = { source: frame.contentWindow, data: { type: "ft.doSomething" } } as unknown as MessageEvent;
    expect(fromFrame(nonsense, frame)).toBeNull();
  });

  it("understands only what a plugin is allowed to ask", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.ready" })).toEqual({ type: "ft.ready" });
    expect(said({ type: "ft.pickFile", id: "q1", accept: "image/*" })).toEqual({
      type: "ft.pickFile",
      id: "q1",
      accept: "image/*",
    });
    expect(said({ type: "ft.made", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toEqual({
      type: "ft.made",
      name: "a.pdf",
      mime: "application/pdf",
      data: "AAA",
    });
    expect(said({ type: "ft.text", text: "hello" })).toEqual({ type: "ft.text", text: "hello" });
    expect(said({ type: "ft.readEverything" })).toBeNull();
    expect(said({ type: "ft.made" })).toBeNull();
  });

  it("understands a plugin asking for a photo from the camera", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.takePhoto", id: "q1" })).toEqual({ type: "ft.takePhoto", id: "q1" });
    expect(said({ type: "ft.takePhoto" })).toBeNull();
    expect(said({ type: "ft.takePhoto", id: 7 })).toBeNull();
  });

  it("understands a plugin's notice", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.notify", text: "Your turn" })).toEqual({ type: "ft.notify", text: "Your turn", sticky: false });
    expect(said({ type: "ft.notify", text: "Waiting…", sticky: true })).toEqual({ type: "ft.notify", text: "Waiting…", sticky: true });
    expect(said({ type: "ft.notify", text: "Hi", sticky: "yes" })).toEqual({ type: "ft.notify", text: "Hi", sticky: false });
    expect(said({ type: "ft.notify", text: "" })).toEqual({ type: "ft.notify", text: "", sticky: false });
    expect(said({ type: "ft.notify", text: "  Your\nturn\u0007 \u009b" })).toEqual({ type: "ft.notify", text: "Your turn", sticky: false });
    expect(said({ type: "ft.notify", text: "\u0000\u001b" })).toEqual({ type: "ft.notify", text: "", sticky: false });

    const long = said({ type: "ft.notify", text: "😀".repeat(500) });
    expect(long && "text" in long && Array.from(long.text)).toHaveLength(NOTICE_LIMIT);
    expect(long && "text" in long && long.text.endsWith("…")).toBe(true);

    expect(said({ type: "ft.notify" })).toBeNull();
    expect(said({ type: "ft.notify", text: 7 })).toBeNull();
  });

  it("understands the questions a plugin asks the core", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.save", id: "q1", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toEqual({
      type: "ft.save",
      id: "q1",
      name: "a.pdf",
      mime: "application/pdf",
      data: "AAA",
    });
    expect(said({ type: "ft.print", id: "q2", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toMatchObject({
      type: "ft.print",
      id: "q2",
    });
    expect(said({ type: "ft.fetch", id: "q3", url: "https://api.openai.com/v1", method: "POST", headers: [], body: null })).toMatchObject({
      type: "ft.fetch",
      id: "q3",
      url: "https://api.openai.com/v1",
      method: "POST",
    });
    expect(said({ type: "ft.read", id: "q4", key: "pen" })).toEqual({ type: "ft.read", id: "q4", key: "pen" });
    expect(said({ type: "ft.write", id: "q5", key: "pen", value: "black" })).toEqual({
      type: "ft.write",
      id: "q5",
      key: "pen",
      value: "black",
    });
    expect(said({ type: "ft.forget", id: "q6", key: "pen" })).toEqual({ type: "ft.forget", id: "q6", key: "pen" });
    expect(said({ type: "ft.close" })).toEqual({ type: "ft.close" });
    expect(said({ type: "ft.closed" })).toEqual({ type: "ft.closed" });

    expect(said({ type: "ft.read", key: "pen" })).toBeNull();
    expect(said({ type: "ft.fetch", id: "q7" })).toBeNull();
    expect(said({ type: "ft.save", id: "q8", name: "a.pdf" })).toBeNull();

    expect(said({ type: "ft.recordSet", id: "r1", key: "note/1", value: "{}" })).toEqual({ type: "ft.recordSet", id: "r1", key: "note/1", value: "{}" });
    expect(said({ type: "ft.recordGet", id: "r2", key: "note/1" })).toEqual({ type: "ft.recordGet", id: "r2", key: "note/1" });
    expect(said({ type: "ft.recordKeys", id: "r3" })).toEqual({ type: "ft.recordKeys", id: "r3", prefix: "" });
    expect(said({ type: "ft.recordUsage", id: "r4" })).toEqual({ type: "ft.recordUsage", id: "r4" });
    expect(said({ type: "ft.remindSet", id: "r5", reminder: "n1", at: 1700, text: "milk" })).toEqual({ type: "ft.remindSet", id: "r5", reminder: "n1", at: 1700, text: "milk" });
    expect(said({ type: "ft.remindSet", id: "r6", reminder: "n1", at: "soon" })).toBeNull();
    expect(said({ type: "ft.remindCancel", id: "r7", reminder: "n1" })).toEqual({ type: "ft.remindCancel", id: "r7", reminder: "n1" });
    expect(said({ type: "ft.liveSend", id: "r8", data: "AQ==" })).toEqual({ type: "ft.liveSend", id: "r8", data: "AQ==" });
    expect(said({ type: "ft.liveSend", id: "r9", data: 7 })).toBeNull();
    expect(said({ type: "ft.openChat", id: "r10", ref: "ref_1" })).toEqual({ type: "ft.openChat", id: "r10", ref: "ref_1" });
    expect(said({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" })).toEqual({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" });
    expect(said({ type: "ft.drive", id: "r12", op: "status" })).toEqual({ type: "ft.drive", id: "r12", op: "status", a: "", b: "" });
    expect(said({ type: "ft.drive", id: "r13", op: "format" })).toBeNull();
    expect(said({ type: "ft.drive", op: "list" })).toBeNull();
    expect(said({ type: "ft.location", id: "l1" })).toEqual({ type: "ft.location", id: "l1" });
    expect(said({ type: "ft.location" })).toBeNull();
    expect(said({ type: "ft.translateLanguages", id: "t1" })).toEqual({ type: "ft.translateLanguages", id: "t1" });
    expect(said({ type: "ft.translateLanguages" })).toBeNull();
    expect(said({ type: "ft.translateDetect", id: "t2", text: "hola" })).toEqual({ type: "ft.translateDetect", id: "t2", text: "hola" });
    expect(said({ type: "ft.translateDetect", id: "t2" })).toBeNull();
    for (const type of ["ft.translateStatus", "ft.translatePrepare", "ft.translateRemove"]) {
      expect(said({ type, id: "t3", from: "es", to: "en", extra: 1 })).toEqual({ type, id: "t3", from: "es", to: "en" });
      expect(said({ type, id: "t3", from: "es" })).toBeNull();
      expect(said({ type, id: "t3", from: 7, to: "en" })).toBeNull();
      expect(said({ type, from: "es", to: "en" })).toBeNull();
    }
    expect(said({ type: "ft.translateText", id: "t4", text: "hola", from: "es", to: "en" })).toEqual({ type: "ft.translateText", id: "t4", text: "hola", from: "es", to: "en" });
    expect(said({ type: "ft.translateText", id: "t4", text: ["hola"], from: "es", to: "en" })).toBeNull();
    expect(said({ type: "ft.translateText", id: "t4", text: "hola", from: "es" })).toBeNull();
  });

  it("understands a hook's answer and a plugin asking to be seen, and nothing more", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) => fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);
    expect(said({ type: "ft.show" })).toEqual({ type: "ft.show" });
    expect(said({ type: "ft.hookDone", id: "h1", result: { text: "hola" } })).toEqual({ type: "ft.hookDone", id: "h1", result: { text: "hola" } });
    expect(said({ type: "ft.hookDone", id: "h2", result: { auto: { message: true, composer: "yes" }, secret: 1 } })).toEqual({
      type: "ft.hookDone",
      id: "h2",
      result: { auto: { message: true } },
    });
    expect(said({ type: "ft.hookDone", id: "h3", result: null })).toEqual({ type: "ft.hookDone", id: "h3", result: null });
    expect(said({ type: "ft.hookDone", id: "h4", result: "hola" })).toEqual({ type: "ft.hookDone", id: "h4", result: null });
    expect(said({ type: "ft.hookDone", id: "h5", result: { text: "x".repeat(5_001) } })).toEqual({ type: "ft.hookDone", id: "h5", result: null });
    expect(said({ type: "ft.hookDone", result: { text: "hola" } })).toBeNull();
    expect(said({ type: "ft.hookDone", id: "h6", result: { lang: "es", auto: { message: true } } })).toEqual({ type: "ft.hookDone", id: "h6", result: { lang: "es", auto: { message: true } } });
    expect(said({ type: "ft.hookDone", id: "h7", result: { lang: "x".repeat(36) } })).toEqual({ type: "ft.hookDone", id: "h7", result: null });
    expect(said({ type: "ft.hookDone", id: "h10", result: { lang: "" } }), "no language any more").toEqual({ type: "ft.hookDone", id: "h10", result: { lang: "" } });
    expect(said({ type: "ft.hookDone", id: "h11", result: { lang: null } }), "none, as the SDK says it").toEqual({ type: "ft.hookDone", id: "h11", result: { lang: null } });
    const group = {
      values: { lang: "fr", receive: true, send: 3, deep: { x: 1 } },
      options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }, { value: "de" }], other: "x" },
      lang: "fr",
    };
    expect(said({ type: "ft.hookDone", id: "h12", result: group })).toEqual({
      type: "ft.hookDone",
      id: "h12",
      result: { values: { lang: "fr", receive: true }, options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }] }, lang: "fr" },
    });
    const many = { options: { lang: Array.from({ length: 400 }, (_, at) => ({ value: `l${at}`, label: `Language ${at}` })) } };
    expect((said({ type: "ft.hookDone", id: "h13", result: many }) as unknown as { result: { options: { lang: unknown[] } } }).result.options.lang, "no list longer than the languages there are").toHaveLength(300);
    expect(said({ type: "ft.hookDone", id: "h8", result: { ask: { text: "Download Spanish,\nabout 30 MB?" } } })).toEqual({
      type: "ft.hookDone",
      id: "h8",
      result: { ask: { text: "Download Spanish, about 30 MB?" } },
    });
    expect(said({ type: "ft.hookDone", id: "h9", result: { ask: { text: "  " } } })).toEqual({ type: "ft.hookDone", id: "h9", result: null });
  });

  it("understands a game reading and writing the player, with its four fields and nothing more", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) => fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);
    const player = { who: "juno", hair: "curly-puff", hairColour: "copper", name: "Ana" };

    expect(said({ type: "ft.playerGet", id: "p1", extra: 1 })).toEqual({ type: "ft.playerGet", id: "p1" });
    expect(said({ type: "ft.playerGet" })).toBeNull();
    expect(said({ type: "ft.playerSet", id: "p2", player })).toEqual({ type: "ft.playerSet", id: "p2", player });
    expect(said({ type: "ft.playerSet", player })).toBeNull();
    for (const wrong of [
      { ...player, vehicle: "bus" },
      { who: "juno", hair: "none", hairColour: "ink" },
      { ...player, hair_colour: "ink", hairColour: undefined },
      { ...player, name: 7 },
      { ...player, name: null },
      null,
      "juno",
      [player],
    ]) {
      expect(said({ type: "ft.playerSet", id: "p3", player: wrong }), JSON.stringify(wrong)).toBeNull();
    }
  });
});
