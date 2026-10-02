// The API a plugin has (plugin-sdk, §53). It is the only way out of the frame: the plugin
// never sees the app, the chat, the keys or another plugin. Everything here is asked for, and the
// core checks what the user granted before doing any of it.
const post = (message) => parent.postMessage(message, "*");
const opened = [];
const waiting = new Map();
let asked = 0;

/** Asks the app for something and waits for its answer, one call at a time. */
const ask = (type, message) => {
  const id = `q${(asked += 1)}`;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    post({ ...message, type, id });
  });
};

globalThis.ft = {
  /** Called when the app opens the plugin: the text the user handed it, and the app's colours. */
  onOpen(handler) {
    opened.push(handler);
  },
  /** Asks the app to ask the user for a file. Resolves with {name, mime, data} or null. */
  pickFile(accept) {
    return ask("ft.pickFile", { accept: accept ?? "" });
  },
  /** Hands a file to the chat; the app sends it. `data` is base64. */
  send(name, mime, data) {
    post({ type: "ft.made", name: String(name), mime: String(mime), data: String(data) });
  },
  /** Puts a text in the composer, for the user to look at before sending it. */
  say(text) {
    post({ type: "ft.text", text: String(text) });
  },
  /** Saves a file on the phone instead of sending it. */
  save(name, mime, data) {
    return ask("ft.save", { name: String(name), mime: String(mime), data: String(data) });
  },
  /** Prints a file, if the user granted this plugin printing. The printer is the user's. */
  print(name, mime, data) {
    return ask("ft.print", { name: String(name), mime: String(mime), data: String(data) });
  },
  /** A call the core makes for the plugin, only to a host the user granted it. */
  fetch(url, options = {}) {
    return ask("ft.fetch", {
      url: String(url),
      method: String(options.method ?? "GET"),
      headers: Object.entries(options.headers ?? {}).map(([name, value]) => [String(name), String(value)]),
      body: options.body ?? null,
    });
  },
  /** What this plugin remembers. Its frame has no storage of its own (§53). */
  store: {
    get: (key) => ask("ft.read", { key: String(key) }),
    set: (key, value) => ask("ft.write", { key: String(key), value: String(value) }),
    forget: (key) => ask("ft.forget", { key: String(key) }),
  },
  /** What this plugin keeps beyond its settings (2026-09-27): notes, boards; within the room the
   *  user granted it. Values are strings (JSON, or base64 for bytes). */
  records: {
    get: (key) => ask("ft.recordGet", { key: String(key) }),
    set: (key, value) => ask("ft.recordSet", { key: String(key), value: String(value) }),
    forget: (key) => ask("ft.recordForget", { key: String(key) }),
    keys: (prefix) => ask("ft.recordKeys", { prefix: String(prefix ?? "") }),
    usage: () => ask("ft.recordUsage", {}),
  },
  /** A notification on this phone at a time this plugin picks (2026-09-27). Needs `remind`. */
  remind: {
    set: (id, at, text) => ask("ft.remindSet", { reminder: String(id), at: Number(at), text: String(text ?? "") }),
    cancel: (id) => ask("ft.remindCancel", { reminder: String(id) }),
    list: () => ask("ft.remindList", {}),
  },
  /** What this plugin says to its twin on the other side of the conversation (2026-09-27), over
   *  the direct connection only. Needs `live`. `data` is base64; what arrives is base64 too. */
  live: {
    send: (data) => ask("ft.liveSend", { data: String(data) }),
    onMessage(handler) {
      heard.push(handler);
    },
  },
  /** The user's own cloud (plan-drive, 2026-09-27). Needs `drive`. Every call resolves with what
   *  the core says, or false when it could not be done; the plugin only ever sees names and sizes.
   *  `status`: {state: none|empty|locked|ready, provider, drive: {files, folders, used, pending,
   *  quota, backupAt}, problem}. `list(parent)`: {folders, files, pending}. */
  drive: {
    status: () => ask("ft.drive", { op: "status" }),
    connect: (provider) => ask("ft.drive", { op: "connect", a: String(provider ?? "google") }),
    setup: () => ask("ft.drive", { op: "setup" }),
    unlock: (code) => ask("ft.drive", { op: "unlock", a: String(code) }),
    disconnect: () => ask("ft.drive", { op: "disconnect" }),
    list: (parent) => ask("ft.drive", { op: "list", a: parent ? String(parent) : "" }),
    mkdir: (name, parent) => ask("ft.drive", { op: "mkdir", a: String(name), b: parent ? String(parent) : "" }),
    rename: (id, name) => ask("ft.drive", { op: "rename", a: String(id), b: String(name) }),
    move: (id, parent) => ask("ft.drive", { op: "move", a: String(id), b: parent ? String(parent) : "" }),
    remove: (id) => ask("ft.drive", { op: "remove", a: String(id) }),
    /** The app opens the picker; what the user picks goes up. Resolves with how many went. */
    upload: (parent) => ask("ft.drive", { op: "upload", a: parent ? String(parent) : "" }),
    /** Keeps the file this plugin was opened with (`onOpen`'s `ref`), without its bytes. */
    keep: (parent) => ask("ft.drive", { op: "keep", a: parent ? String(parent) : "" }),
    open: (id) => ask("ft.drive", { op: "open", a: String(id) }),
    save: (id) => ask("ft.drive", { op: "save", a: String(id) }),
    /** Sends a file of the drive to the conversation, as the `send` permission allows. */
    send: (id) => ask("ft.drive", { op: "send", a: String(id) }),
    retry: () => ask("ft.drive", { op: "retry" }),
    cancel: (blob) => ask("ft.drive", { op: "cancel", a: String(blob) }),
    backup: () => ask("ft.drive", { op: "backup" }),
    backupInfo: () => ask("ft.drive", { op: "backupInfo" }),
    restore: () => ask("ft.drive", { op: "restore" }),
  },
  /** The phone's current position, once, while the app is open (2026-10-02). Needs `location`.
   *  Resolves {lat, lon, accuracy (metres), at (ms)}, or null: refused, off or no fix in ~15 s. */
  location: () => ask("ft.location", {}).then((fix) =>
    fix && typeof fix === "object" && Number.isFinite(fix.lat) && Number.isFinite(fix.lon)
      ? { lat: Number(fix.lat), lon: Number(fix.lon), accuracy: Number(fix.accuracy), at: Number(fix.at) }
      : null,
  ),
  /** Goes back to the conversation a `ref` came from. Resolves false if it is gone. */
  openChat(ref) {
    return ask("ft.openChat", { ref: String(ref) });
  },
  /** Closes the plugin's window. */
  close() {
    post({ type: "ft.close" });
  },
};
const heard = [];

// The app's colours (2026-10-02): Ionic's variables on the root, so a plugin's
// `var(--ion-text-color, …)` follows the app, dark or light. Only these names, and only what looks
// like a colour: nothing else of the app reaches the plugin this way.
const COLOURS = [
  "--ion-background-color",
  "--ion-text-color",
  "--ion-color-medium",
  "--ion-item-background",
  "--ion-border-color",
  "--ion-color-primary",
  "--ion-color-primary-contrast",
  "--ion-color-success",
  "--ion-color-danger",
];
const COLOUR = /^(#[0-9a-f]{3,8}|(rgb|rgba|hsl|hsla)\([-+0-9.,%\/\sdeg]*\))$/i;
let theme = {};
const paint = (said) => {
  const given = said.theme && typeof said.theme === "object" ? said.theme : {};
  theme = {};
  for (const name of COLOURS) {
    const value = typeof given[name] === "string" ? given[name].trim() : "";
    if (COLOUR.test(value)) theme[name] = value;
  }
  const root = document.documentElement;
  for (const name of COLOURS) {
    if (name in theme) root.style.setProperty(name, theme[name]);
    else root.style.removeProperty(name);
  }
  if (said.dark) root.dataset.dark = "1";
  else delete root.dataset.dark;
  root.style.colorScheme = said.dark ? "dark" : "light";
};

// The colours are there before the plugin draws anything: asked for, and waited for a moment only.
await new Promise((resolve) => {
  const first = (event) => {
    if (!event.data || event.data.type !== "ft.theme") return;
    removeEventListener("message", first);
    paint(event.data);
    resolve();
  };
  addEventListener("message", first);
  post({ type: "ft.hello" });
  setTimeout(() => {
    removeEventListener("message", first);
    resolve();
  }, 1000);
});

await import("./dist/index.js");

const fallback = document.getElementById("fallback");
if (fallback) fallback.remove();

const view = document.getElementById("view");
// How tall the plugin's content is (2026-10-04): the bottom of the body, its margin included. Not
// the root's scrollHeight, which is never less than the frame's own height: a frame measured that
// way grows with its content but never comes back down.
const tell = () => {
  const body = document.body;
  const margin = parseFloat(getComputedStyle(body).marginBottom) || 0;
  post({ type: "ft.height", height: Math.ceil(body.getBoundingClientRect().bottom + scrollY + margin) });
};

addEventListener("message", (event) => {
  const said = event.data;
  if (!said || typeof said.type !== "string") return;
  if (said.type === "ft.open") {
    const text = String(said.text ?? "");
    if (view) view.setAttribute("text", text);
    paint(said);
    const lang = String(said.lang ?? "en");
    document.documentElement.lang = lang;
    const file = said.file && said.file.name ? { name: String(said.file.name), mime: String(said.file.mime), data: String(said.file.data) } : null;
    const opening = {
      text,
      dark: Boolean(said.dark),
      lang,
      file,
      ref: said.ref ? String(said.ref) : null,
      reminder: said.reminder ? String(said.reminder) : null,
      live: Boolean(said.live),
      // The app's colours as they were put on the root, for a plugin that paints on a canvas.
      theme: { ...theme },
      // The chat it was opened in (2026-10-02): an opaque id, only when there is one.
      ...(typeof said.chat === "string" ? { chat: said.chat } : {}),
    };
    for (const handler of opened) handler(opening);
    requestAnimationFrame(tell);
  } else if (said.type === "ft.theme") {
    paint(said);
  } else if (said.type === "ft.live") {
    for (const handler of heard) handler(String(said.data ?? ""));
  } else if (said.type === "ft.file") {
    const answer = waiting.get(said.id);
    waiting.delete(said.id);
    if (answer) answer(said.name ? { name: said.name, mime: said.mime, data: said.data } : null);
  } else if (said.type === "ft.done") {
    const answer = waiting.get(said.id);
    waiting.delete(said.id);
    if (answer) answer(said.answer ?? null);
  }
});

const sizes = new ResizeObserver(tell);
sizes.observe(document.documentElement);
sizes.observe(document.body);
post({ type: "ft.ready" });
