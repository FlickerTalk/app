/**
 * A stand-in for the Rust core, for the end-to-end tests of the web UI: `window.__TAURI_INTERNALS__`
 * answered by an in-memory state, injected before the app loads. The real `@tauri-apps/api` code
 * runs; only what is behind the bridge is faked. Events the core would emit (`ft://changed`) are
 * emitted with `window.__ftFake.emit`, and every command the UI sent is kept in
 * `window.__ftFake.calls`, so a test can assert what reached the core.
 *
 * Everything is one function, serialised into the page by Playwright: it may import nothing.
 */
export function installFakeCore() {
  type Args = Record<string, unknown> | undefined;
  type Handler = (event: { event: string; payload: unknown; id: number }) => void;

  const conversation = (id: string, name: string, text: string, unread = 0) => ({
    id,
    name,
    unread,
    blocked: false,
    connected: false,
    last: { id: `${id}-last`, outgoing: false, text, sentAt: Date.now(), state: "delivered" },
  });

  const state = {
    autoDownload: 10 * 1024 * 1024,
    conversations: [conversation("ft_bob123456789", "Bob", "see you at six")],
    requests: [conversation("ft_stranger12345", "Mamá", "hey, it's me, new number", 1)],
    sessions: {} as Record<string, { id: string; pin: string }>,
    open: [] as string[],
    messages: {
      ft_bob123456789: [
        { id: "m1", outgoing: false, text: "see you at six", sentAt: Date.now() - 60_000, state: "delivered" },
        {
          id: "big",
          outgoing: false,
          text: "holiday-photos.zip",
          sentAt: Date.now() - 30_000,
          state: "delivered",
          file: { name: "holiday-photos.zip", size: 48_000_000, mime: "application/zip", progress: 0, state: "waiting", path: "/x/big" },
        },
      ],
    } as Record<string, unknown[]>,
    plugins: [
      {
        id: "com.flickertalk.markdown",
        name: "Markdown",
        version: "1.0.0",
        asks: { network: [], messages: false, send: "propose" },
        granted: { network: [], messages: false, send: "propose" },
        installedAt: Date.now(),
      },
      {
        id: "com.flickertalk.sketch",
        name: "Sketch",
        version: "1.0.0",
        asks: { network: [], messages: false, send: "propose" },
        granted: { network: [], messages: false, send: "nothing" },
        installedAt: Date.now(),
      },
    ],
    nextSession: 1,
  };

  const handlers = new Map<number, Handler>();
  const listeners = new Map<string, number[]>();
  let nextCallback = 1;
  const calls: Array<[string, Args]> = [];

  const sessionView = (id: string) => ({ id, conversations: [], requests: [] });

  const answer = (command: string, args: Args): unknown => {
    const a = (args ?? {}) as Record<string, string | number | undefined>;
    switch (command) {
      case "plugin:event|listen": {
        const name = String(a.event);
        listeners.set(name, [...(listeners.get(name) ?? []), Number(a.handler)]);
        return nextCallback;
      }
      case "plugin:event|unlisten":
        return undefined;
      case "plugin:app|version":
        return "1.0.0-e2e";
      case "core_me":
        return { id: "ft_me", name: "Me", mailbox: true, receipts: true, freeUntil: Date.now() + 1e10, autoDownload: state.autoDownload };
      case "core_conversations":
        return state.conversations;
      case "core_requests":
        return state.requests;
      case "core_sessions":
        return state.open.map(sessionView);
      case "core_messages":
        return state.messages[String(a.contact)] ?? [];
      case "core_plugins":
        return state.plugins;
      case "core_catalogue":
      case "core_calls":
        return [];
      case "core_card":
        return "https://flickertalk.com/add#card";
      case "core_pending_call":
        return "";
      case "core_quiet_hours":
        return null;
      case "core_plan":
        return { state: "trial", until: Date.now() + 1e10, age: "unknown" };
      // Like the core (A3): every PIN opens its session or a new empty one, up to seven; an
      // empty one goes when it is closed, and the fake's sessions are always empty.
      case "core_session_open": {
        const pin = String(a.pin);
        let found = Object.values(state.sessions).find((s) => s.pin === pin);
        if (!found) {
          if (Object.keys(state.sessions).length >= 7) return null;
          found = { id: `s${state.nextSession++}`, pin };
          state.sessions[found.id] = found;
        }
        if (!state.open.includes(found.id)) state.open.push(found.id);
        return sessionView(found.id);
      }
      case "core_session_close":
        delete state.sessions[String(a.session)];
        state.open = state.open.filter((id) => id !== a.session);
        return undefined;
      case "core_session_remove":
        delete state.sessions[String(a.session)];
        state.open = state.open.filter((id) => id !== a.session);
        return undefined;
      case "core_accept_contact": {
        const index = state.requests.findIndex((r) => r.id === a.contact);
        if (index >= 0) state.conversations.push(...state.requests.splice(index, 1));
        return undefined;
      }
      case "core_decline_contact":
        state.requests = state.requests.filter((r) => r.id !== a.contact);
        return undefined;
      case "core_accept_file": {
        for (const list of Object.values(state.messages)) {
          for (const message of list as Array<{ id: string; file?: { state: string; progress: number } }>) {
            if (message.id === a.message && message.file) {
              message.file.state = "transferring";
              message.file.progress = 0.25;
            }
          }
        }
        // The core would announce the change once chunks arrive.
        setTimeout(() => emit("ft://changed", { contact: "ft_bob123456789" }), 10);
        return undefined;
      }
      case "core_set_auto_download":
        state.autoDownload = Number(a.bytes);
        return undefined;
      case "core_renew_link":
        return "https://flickertalk.com/add#renewed";
      case "core_plugin_made": {
        const plugin = state.plugins.find((p) => p.id === a.plugin);
        if (!plugin || plugin.granted.send === "nothing") throw new Error("that plugin may not write in the chat");
        if (plugin.granted.send === "auto") return { sent: true };
        return { sent: false, staged: { path: `/data/files/outgoing/1-${a.name}`, name: a.name, mime: a.mime, size: 3 } };
      }
      case "core_send_picked":
      case "core_send":
      case "core_mark_read":
      case "core_enable_push":
      default:
        return undefined;
    }
  };

  const emit = (event: string, payload: unknown) => {
    for (const id of listeners.get(event) ?? []) {
      handlers.get(id)?.({ event, payload, id });
    }
  };

  const fake = {
    calls,
    state,
    emit,
    invoke: (command: string, args?: Args) => {
      calls.push([command, args]);
      try {
        return Promise.resolve(answer(command, args));
      } catch (error) {
        return Promise.reject(error);
      }
    },
    convertFileSrc: (path: string, protocol = "asset") => `http://${protocol}.localhost/${path}`,
    transformCallback: (callback: Handler) => {
      const id = nextCallback++;
      handlers.set(id, callback);
      return id;
    },
    unregisterCallback: (id: number) => {
      handlers.delete(id);
    },
  };

  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = fake;
  (window as unknown as { __ftFake: unknown }).__ftFake = fake;
  // The welcome is over: straight to the chats.
  try {
    localStorage.setItem("ft-onboarded", "1");
  } catch {
    // A page without storage still starts.
  }
}
