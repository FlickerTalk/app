const post = (message) => parent.postMessage(message, "*");
const opened = [];
const waiting = new Map();
let asked = 0;

const textOrNull = (said) => (typeof said === "string" ? said : null);

const ask = (type, message) => {
  const id = `q${(asked += 1)}`;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    post({ ...message, type, id });
  });
};

globalThis.ft = {
  onOpen(handler) {
    opened.push(handler);
  },
  pickFile(accept) {
    return ask("ft.pickFile", { accept: accept ?? "" });
  },
  takePhoto: () => ask("ft.takePhoto", {}),
  send(name, mime, data) {
    post({ type: "ft.made", name: String(name), mime: String(mime), data: String(data) });
  },
  say(text) {
    post({ type: "ft.text", text: String(text) });
  },
  notify(text, options) {
    post({ type: "ft.notify", text: String(text ?? ""), sticky: options?.sticky === true });
  },
  save(name, mime, data) {
    return ask("ft.save", { name: String(name), mime: String(mime), data: String(data) });
  },
  print(name, mime, data) {
    return ask("ft.print", { name: String(name), mime: String(mime), data: String(data) });
  },
  fetch(url, options = {}) {
    return ask("ft.fetch", {
      url: String(url),
      method: String(options.method ?? "GET"),
      headers: Object.entries(options.headers ?? {}).map(([name, value]) => [String(name), String(value)]),
      body: options.body ?? null,
    });
  },
  store: {
    get: (key) => ask("ft.read", { key: String(key) }),
    set: (key, value) => ask("ft.write", { key: String(key), value: String(value) }),
    forget: (key) => ask("ft.forget", { key: String(key) }),
  },
  records: {
    get: (key) => ask("ft.recordGet", { key: String(key) }),
    set: (key, value) => ask("ft.recordSet", { key: String(key), value: String(value) }),
    forget: (key) => ask("ft.recordForget", { key: String(key) }),
    keys: (prefix) => ask("ft.recordKeys", { prefix: String(prefix ?? "") }),
    usage: () => ask("ft.recordUsage", {}),
  },
  remind: {
    set: (id, at, text) => ask("ft.remindSet", { reminder: String(id), at: Number(at), text: String(text ?? "") }),
    cancel: (id) => ask("ft.remindCancel", { reminder: String(id) }),
    list: () => ask("ft.remindList", {}),
  },
  live: {
    send: (data) => ask("ft.liveSend", { data: String(data) }),
    onMessage(handler) {
      heard.push(handler);
    },
  },
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
    upload: (parent) => ask("ft.drive", { op: "upload", a: parent ? String(parent) : "" }),
    keep: (parent) => ask("ft.drive", { op: "keep", a: parent ? String(parent) : "" }),
    open: (id) => ask("ft.drive", { op: "open", a: String(id) }),
    save: (id) => ask("ft.drive", { op: "save", a: String(id) }),
    send: (id) => ask("ft.drive", { op: "send", a: String(id) }),
    retry: () => ask("ft.drive", { op: "retry" }),
    cancel: (blob) => ask("ft.drive", { op: "cancel", a: String(blob) }),
    backup: () => ask("ft.drive", { op: "backup" }),
    backupInfo: () => ask("ft.drive", { op: "backupInfo" }),
    restore: () => ask("ft.drive", { op: "restore" }),
  },
  location: () => ask("ft.location", {}).then((fix) =>
    fix && typeof fix === "object" && Number.isFinite(fix.lat) && Number.isFinite(fix.lon)
      ? { lat: Number(fix.lat), lon: Number(fix.lon), accuracy: Number(fix.accuracy), at: Number(fix.at) }
      : null,
  ),
  translate: {
    languages: () => ask("ft.translateLanguages", {}).then((tags) => (Array.isArray(tags) ? tags.filter((tag) => typeof tag === "string") : [])),
    detect: (text) => ask("ft.translateDetect", { text: String(text) }).then(textOrNull),
    status: (from, to) =>
      ask("ft.translateStatus", { from: String(from), to: String(to) }).then((status) =>
        status === "installed" || status === "downloadable" ? status : "unsupported",
      ),
    prepare: (from, to) => ask("ft.translatePrepare", { from: String(from), to: String(to) }).then((ready) => ready === true),
    remove: (from, to) => ask("ft.translateRemove", { from: String(from), to: String(to) }).then((removed) => removed === true),
    translate: (text, from, to) => ask("ft.translateText", { text: String(text), from: String(from), to: String(to) }).then(textOrNull),
  },
  openChat(ref) {
    return ask("ft.openChat", { ref: String(ref) });
  },
  close() {
    post({ type: "ft.close" });
  },
  onClose(handler) {
    closers.push(handler);
  },
  onHook(handler) {
    hooked = handler;
  },
  show() {
    post({ type: "ft.show" });
  },
};
const heard = [];
const closers = [];
let hooked;
const HOOKS = ["message", "composer", "settings"];
const PLATFORMS = ["android", "ios", "desktop"];

const settingValues = (said) => {
  if (!said || typeof said !== "object" || Array.isArray(said)) return null;
  const values = {};
  for (const [key, value] of Object.entries(said)) if (typeof value === "string" || typeof value === "boolean") values[key] = value;
  return values;
};

const settingOptions = (said) => {
  if (!said || typeof said !== "object" || Array.isArray(said)) return null;
  const options = {};
  for (const [key, list] of Object.entries(said)) {
    if (!Array.isArray(list)) continue;
    options[key] = list
      .filter((one) => one && typeof one.value === "string" && typeof one.label === "string")
      .map((one) => ({ value: one.value, label: one.label }));
  }
  return options;
};

const hookResult = (said) => {
  if (!said || typeof said !== "object") return null;
  const result = {};
  if (typeof said.text === "string") result.text = said.text;
  if (typeof said.lang === "string" || said.lang === null) result.lang = said.lang;
  if (said.ask && typeof said.ask === "object" && typeof said.ask.text === "string") result.ask = { text: said.ask.text };
  const values = settingValues(said.values);
  if (values) result.values = values;
  const options = settingOptions(said.options);
  if (options) result.options = options;
  if (said.auto && typeof said.auto === "object") {
    const auto = {};
    for (const name of ["message", "composer"]) if (typeof said.auto[name] === "boolean") auto[name] = said.auto[name];
    result.auto = auto;
  }
  return Object.keys(result).length ? result : null;
};

const runHook = async (said) => {
  let result = null;
  if (hooked && HOOKS.includes(said.name)) {
    const hook = {
      name: said.name,
      text: String(said.text ?? ""),
      chat: String(said.chat ?? ""),
      ref: said.ref ? String(said.ref) : null,
      lang: String(said.lang ?? "en"),
      manual: said.manual === true,
      ...(PLATFORMS.includes(said.platform) ? { platform: said.platform } : {}),
      ...(said.answer === true ? { answer: true } : {}),
      ...(settingValues(said.values) ? { values: settingValues(said.values) } : {}),
    };
    try {
      result = hookResult(await hooked(hook));
    } catch {
      result = null;
    }
  }
  post({ type: "ft.hookDone", id: String(said.id ?? ""), result });
};
let goodbye;

// Only these variables, and only values that look like a colour, reach the plugin.
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
  if (said.dir === "rtl" || said.dir === "ltr") root.dir = said.dir;
  if (said.fill === true) root.dataset.fill = "1";
  else delete root.dataset.fill;
};

const lent = document.documentElement.dataset.ionic;

// Get the colours before the plugin draws anything, but wait a second at most.
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

// Same ?v= as the frame, so a cache never answers an update with the old code.
await import(`./dist/index.js${new URL(import.meta.url).search}`);

const fallback = document.getElementById("fallback");
if (fallback) fallback.remove();

const view = document.getElementById("view");
// Measure the body's bottom, not the root's scrollHeight. scrollHeight never drops below the
// frame's own height, so the frame would grow with its content but never shrink back.
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
      theme: { ...theme },
      ...(typeof said.chat === "string" ? { chat: said.chat } : {}),
      ...(lent ? { ionic: { version: lent } } : {}),
      ...(said.presenting === "lead" || said.presenting === "follow" ? { presenting: said.presenting } : {}),
      ...(HOOKS.includes(said.hook) ? { hook: said.hook } : {}),
    };
    for (const handler of opened) handler(opening);
    requestAnimationFrame(tell);
  } else if (said.type === "ft.theme") {
    paint(said);
  } else if (said.type === "ft.hook") {
    void runHook(said);
  } else if (said.type === "ft.live") {
    for (const handler of heard) handler(String(said.data ?? ""));
  } else if (said.type === "ft.closing") {
    // allSettled so a handler that throws or hangs doesn't stop the others. The app cuts off one
    // that never ends.
    goodbye ??= Promise.allSettled(closers.map(async (handler) => handler()));
    goodbye.then(() => post({ type: "ft.closed" }));
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
