/**
 * A stand-in for the Rust core, for the end-to-end tests of the web UI: `window.__TAURI_INTERNALS__`
 * answered by an in-memory state, injected before the app loads. The real `@tauri-apps/api` code
 * runs; only what is behind the bridge is faked. Events the core would emit (`ft://changed`) are
 * emitted with `window.__ftFake.emit`, and every command the UI sent is kept in
 * `window.__ftFake.calls`, so a test can assert what reached the core.
 *
 * Calls stay on the WebView unless a test sets `window.__ftFakeNative` before the app loads: then
 * the fake plays a phone whose calls are native (docs/video-nativo.md), connects at once, and keeps
 * both cameras in `state.video`. `window.__ftFakeCameraDenied` makes turning the camera on fail as
 * a denied permission does; `window.__ftFakeCameraFails` makes the camera fail to start (an
 * encoder that cannot be set up), as the real core does: with a `camera_failed` event when a
 * video call connects, with an error when it is turned on.
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
    // Circles (2026-09-27): one of Bob and me, made by me.
    circles: [
      {
        id: "circle1",
        name: "Friends",
        members: [
          { id: "ft_me", name: "Me", admin: true, me: true },
          { id: "ft_bob123456789", name: "Bob", admin: false, me: false },
        ],
        admin: true,
        adminsOnly: false,
        left: false,
        unread: 1,
        last: { id: "cm1", outgoing: false, sender: "ft_bob123456789", senderName: "Bob", kind: "text", text: "dinner on friday?", sentAt: Date.now(), state: "delivered" },
      },
    ] as Array<Record<string, unknown> & { id: string; name: string; members: Array<{ id: string; name: string; admin: boolean; me: boolean }>; left: boolean; adminsOnly: boolean }>,
    circleMessages: {
      circle1: [
        { id: "ce1", outgoing: true, sender: "ft_me", senderName: "Me", kind: "created", text: "Friends", sentAt: Date.now() - 120_000, state: "read" },
        { id: "cm1", outgoing: false, sender: "ft_bob123456789", senderName: "Bob", kind: "text", text: "dinner on friday?", sentAt: Date.now() - 60_000, state: "delivered" },
      ],
    } as Record<string, unknown[]>,
    nextCircle: 2,
    // The native call's cameras, as the core's `kind: "video"` snapshot.
    nativeContact: "",
    // The native call's phase, for `core_current_call`; "" when there is none.
    nativePhase: "",
    connectedAt: 0,
    video: { available: false, camera: false, paused: false, facing: "front", remote: false, remotePaused: false },
  };
  const flag = (name: string) => Boolean((window as unknown as Record<string, unknown>)[name]);
  const NATIVE_CALL = "call-e2e";
  const callEvent = (payload: Record<string, unknown>) =>
    emit("ft://call", { contact: String(state.nativeContact), call: NATIVE_CALL, ...payload });

  const handlers = new Map<number, Handler>();
  const listeners = new Map<string, number[]>();
  let nextCallback = 1;
  const calls: Array<[string, Args]> = [];

  const sessionView = (id: string) => ({ id, conversations: [], requests: [], circles: [] });

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
      // Native calls (2026-09-28, video since 2026-09-29): the fake is a browser, so calls stay on
      // the WebView unless a test plays a phone; no call is going on when the app starts.
      case "core_native_calls":
        return flag("__ftFakeNative");
      case "core_current_call":
        return state.nativePhase
          ? {
              call: NATIVE_CALL,
              contact: state.nativeContact,
              video: { ...state.video },
              outgoing: true,
              phase: state.nativePhase,
              native: true,
              muted: false,
              connectedAt: state.connectedAt || undefined,
            }
          : null;
      case "core_call_start_native": {
        // The other side answers at once and the call connects with a video line both ways. Until
        // the video is ready the core's camera is the wish. As the real core, right after
        // `connected` it says the video it has (`available`, the camera off), then the camera it
        // turned on, or `camera_failed` when it could not start.
        state.nativeContact = String(a.contact);
        state.nativePhase = "calling";
        const wanted = Boolean(args?.video);
        Object.assign(state.video, { available: false, camera: wanted, paused: false, facing: "front", remote: false, remotePaused: false });
        setTimeout(() => {
          state.nativePhase = "connecting";
          callEvent({ kind: "answered" });
        }, 10);
        setTimeout(() => {
          state.nativePhase = "active";
          state.connectedAt = Date.now();
          callEvent({ kind: "connected" });
        }, 20);
        setTimeout(() => {
          Object.assign(state.video, { available: true, camera: false });
          callEvent({ kind: "video", ...state.video });
          if (!wanted) return;
          if (flag("__ftFakeCameraFails")) {
            callEvent({ kind: "camera_failed" });
            return;
          }
          state.video.camera = true;
          callEvent({ kind: "video", ...state.video });
        }, 30);
        return NATIVE_CALL;
      }
      case "core_call_set_video":
        if (args?.on && flag("__ftFakeCameraDenied")) throw "camera_denied";
        if (args?.on && flag("__ftFakeCameraFails")) throw "the camera cannot start: configure failed";
        state.video.camera = Boolean(args?.on);
        setTimeout(() => callEvent({ kind: "video", ...state.video }), 5);
        return { ...state.video };
      case "core_call_switch_camera":
        state.video.facing = state.video.facing === "front" ? "back" : "front";
        return { ...state.video };
      case "core_call_end":
        if (a.call === NATIVE_CALL) {
          state.nativePhase = "";
          setTimeout(() => callEvent({ kind: "ended", outcome: "answered" }), 5);
        }
        return undefined;
      case "core_set_call_routing":
      case "core_call_answer_native":
      case "core_call_video_layout":
      case "core_call_mute":
      case "core_call_speaker":
        return undefined;
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
      case "core_circles":
        return state.circles;
      case "core_circle_messages":
        return state.circleMessages[String(a.circle)] ?? [];
      case "core_circle_create": {
        const id = `circle${state.nextCircle++}`;
        const members = (args?.members as string[]).map((member) => ({
          id: member,
          name: state.conversations.find((c) => c.id === member)?.name ?? member,
          admin: false,
          me: false,
        }));
        state.circles.push({
          id,
          name: String(a.name),
          members: [{ id: "ft_me", name: "Me", admin: true, me: true }, ...members],
          admin: true,
          adminsOnly: false,
          left: false,
          unread: 0,
          last: null,
        });
        state.circleMessages[id] = [{ id: `${id}-e1`, outgoing: true, sender: "ft_me", senderName: "Me", kind: "created", text: String(a.name), sentAt: Date.now(), state: "read" }];
        return id;
      }
      case "core_circle_send": {
        const id = String(a.circle);
        const message = { id: `${id}-${Date.now()}`, outgoing: true, sender: "ft_me", senderName: "Me", kind: "text", text: String(a.text), sentAt: Date.now(), state: "sent" };
        state.circleMessages[id] = [...(state.circleMessages[id] ?? []), message];
        const circle = state.circles.find((c) => c.id === id);
        if (circle) circle.last = message;
        setTimeout(() => emit("ft://changed", { contact: null, circle: id }), 10);
        return undefined;
      }
      case "core_circle_leave": {
        const circle = state.circles.find((c) => c.id === a.circle);
        if (circle) circle.left = true;
        return undefined;
      }
      case "core_circle_forget":
        state.circles = state.circles.filter((c) => c.id !== a.circle);
        return undefined;
      case "core_circle_admins_only": {
        const circle = state.circles.find((c) => c.id === a.circle);
        if (circle) circle.adminsOnly = Boolean(a.adminsOnly);
        return undefined;
      }
      case "core_circle_invite": {
        const circle = state.circles.find((c) => c.id === a.circle);
        const contact = state.conversations.find((c) => c.id === a.contact);
        if (circle && contact) circle.members.push({ id: contact.id, name: contact.name, admin: false, me: false });
        return undefined;
      }
      case "core_circle_remove": {
        const circle = state.circles.find((c) => c.id === a.circle);
        if (circle) circle.members = circle.members.filter((member) => member.id !== a.contact);
        return undefined;
      }
      case "core_circle_mark_read": {
        const circle = state.circles.find((c) => c.id === a.circle);
        if (circle) circle.unread = 0;
        return undefined;
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
