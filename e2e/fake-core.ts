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
 * The camera scanner: `window.__ftFakeScanCamera` (`{ before, answer, after }`, all `granted` by
 * default) is the camera permission as the system reports it before asking, the answer to asking,
 * and after it. `scan` waits until `window.__ftFake.scanned(text)` or `cancel`.
 *
 * Android's back button: `window.__ftFake.back()` presses it; it returns whether the app was
 * listening (`onBackButtonPress`), `false` when the system would do its usual job.
 * `window.__ftFake.listening()` says whether it listens, without pressing.
 *
 * `window.__ftFakeMeId` gives this phone a real-length FlickerTalk ID instead of `ft_me`.
 *
 * `window.__ftFakeLongChat` (a number) puts that many older texts before Bob's messages, for a
 * conversation longer than the screen.
 *
 * Hidden sessions (2026-10-01, §108): the PIN `777777` opens, the first time, a session with a
 * contact in it (Ana, `ft_hidden1234567`), so it stays when it is closed, as the core's would. What
 * plugins keep (records, settings, reminders) is kept as the core keeps it: apart for each place
 * (the main list, or one session), refused for a session that is not open, and gone with its
 * session.
 *
 * The other phone may ring this one too (`window.__ftFake.ring`): as the real core since
 * 2026-09-29, whoever answers (the app's button, `core_call_answer_native`, or the phone's own call
 * screen, `window.__ftFake.phoneAnswers`), the core answers and says `answering`, then
 * `connected`; a call answered before its offer came arrives as `incoming` with `answered`.
 *
 * A plugin opened in a chat gets a deterministic id of it (`core_plugin_chat`, 2026-10-02), its own
 * for each plugin and contact, refused for a contact that is blocked or in a closed session.
 *
 * Games (plan 10, app 1.3.0): one game is installed (Tic-tac-toe, granted nothing yet) and the
 * catalogue offers another (Chess); installing, granting and removing change the fake's lists as
 * the core would. `window.__ftFakeInstallFails` makes installing fail, as a download would offline.
 * `window.__ftFakeBobSays` (texts) adds Bob's messages after his others: an invitation, say.
 * `window.__ftFakeISaid` (texts) adds messages of this phone to Bob, read, after Bob's.
 * `window.__ftFakeFiles` (`{ outgoing, name, mime, state }`) adds files to Bob's conversation, last.
 * `window.__ftFakeBobName` renames Bob (a long name, to see the chat header truncate it).
 * `window.__ftFakeBobLast` replaces the text of Bob's last message (a long preview in the list).
 * `window.__ftFakeCarol` adds a second contact, Carol (`ft_carol12345678`), and a plugin's
 * `ft.openChat` then leads to her conversation.
 * `window.__ftFakeManyPlugins` (a number) installs that many more tools and as many games, for a
 * list longer than the screen.
 * `window.__ftFakeCatalogue` (entries) adds to what the catalogue lists, with their `locales`
 * (2026-10-02): entries for installed plugins too, as the real core lists them.
 *
 * The live channel (2026-10-02): `window.__ftFakeLivePlugins` (ids) start with `live` granted, and
 * `core_plugin_live_send` takes what such a plugin says to its twin (kept in `calls`), as the core
 * would over a direct connection; one without the grant is refused.
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
    /** The conversations of each session; only the one of PIN 777777 has any. */
    sessionChats: {} as Record<string, ReturnType<typeof conversation>[]>,
    seededHidden: false,
    /** What plugins keep, by `kind|plugin|session|key` ("" for the main list). */
    pluginData: {} as Record<string, string>,
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
      {
        id: "com.flickertalk.game.tictactoe",
        name: "Tic-tac-toe",
        version: "1.0.0",
        kind: "game",
        asks: { network: [], messages: false, send: "propose", live: true },
        granted: { network: [], messages: false, send: "nothing", live: false },
        installedAt: Date.now(),
      },
    ] as Array<Record<string, unknown> & { id: string; name: string; granted: Record<string, unknown> }>,
    /** What the catalogue offers besides what is installed (2026-10-02, plan 10). */
    catalogue: [
      {
        id: "com.flickertalk.game.chess",
        name: "Chess",
        version: "1.0.0",
        summary: "Chess for two, move by move.",
        size: 412_000,
        carried: false,
        kind: "game",
        locales: { es: { name: "Ajedrez", summary: "Ajedrez para dos, jugada a jugada." } },
      },
    ] as Array<Record<string, unknown> & { id: string; name: string; version: string; kind: string }>,
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
    nativeOutgoing: true,
    connectedAt: 0,
    video: { available: false, camera: false, paused: false, facing: "front", remote: false, remotePaused: false },
  };
  const flag = (name: string) => Boolean((window as unknown as Record<string, unknown>)[name]);
  const catalogue = () => [...state.catalogue, ...(((window as unknown as Record<string, unknown>).__ftFakeCatalogue as typeof state.catalogue) ?? [])];
  /** Read when asked, as the other knobs are: a test may set it after the fake is installed. */
  const grantLive = () => {
    const ids = ((window as unknown as Record<string, unknown>).__ftFakeLivePlugins as string[] | undefined) ?? [];
    for (const plugin of state.plugins) if (ids.includes(plugin.id)) plugin.granted = { ...plugin.granted, live: true };
  };
  const NATIVE_CALL = "call-e2e";
  const callEvent = (payload: Record<string, unknown>) =>
    emit("ft://call", { contact: String(state.nativeContact), call: NATIVE_CALL, ...payload });

  const handlers = new Map<number, Handler>();
  const listeners = new Map<string, number[]>();
  let nextCallback = 1;
  const calls: Array<[string, Args]> = [];

  // The camera scanner and Android's back button.
  const scanCamera = () =>
    ({ before: "granted", answer: "granted", after: "granted", ...((window as unknown as Record<string, unknown>).__ftFakeScanCamera as object) }) as {
      before: string;
      answer: string;
      after: string;
    };
  let cameraAsked = false;
  let scanning: { resolve: (read: unknown) => void; reject: (reason: unknown) => void } | null = null;
  let back: { channel: number; index: number } | null = null;

  const sessionView = (id: string) => ({ id, conversations: state.sessionChats[id] ?? [], requests: [], circles: [] });

  // Where a plugin is open, as the core checks it: the main list (""), or a session that is open.
  const place = (a: Record<string, unknown>) => {
    const session = a.session ? String(a.session) : "";
    if (session && !state.open.includes(session)) throw new Error("that session is not open");
    return session;
  };
  const dataKey = (kind: string, a: Record<string, unknown>, key: unknown) => `${kind}|${String(a.plugin)}|${place(a)}|${String(key)}`;
  const dataKeys = (kind: string, a: Record<string, unknown>, prefix = "") => {
    const start = `${kind}|${String(a.plugin)}|${place(a)}|`;
    return Object.keys(state.pluginData)
      .filter((key) => key.startsWith(start + prefix))
      .map((key) => key.slice(start.length))
      .sort();
  };
  const forgetSession = (id: string) => {
    delete state.sessions[id];
    delete state.sessionChats[id];
    for (const key of Object.keys(state.pluginData)) if (key.split("|")[2] === id) delete state.pluginData[key];
  };

  // The sessions, and what plugins keep, survive a reload, as the core keeps them on disk: a reload
  // is the app starting again. Each test has a fresh browser context, so a fresh disk.
  const KEPT_SESSIONS = "ft-fake-sessions";
  const keepSessions = () => {
    try {
      localStorage.setItem(
        KEPT_SESSIONS,
        JSON.stringify({
          sessions: state.sessions,
          open: state.open,
          next: state.nextSession,
          chats: state.sessionChats,
          seeded: state.seededHidden,
          data: state.pluginData,
        }),
      );
    } catch {
      // A page without storage forgets them, like a phone erased.
    }
  };
  try {
    const kept = JSON.parse(localStorage.getItem(KEPT_SESSIONS) ?? "null") as {
      sessions: typeof state.sessions;
      open: string[];
      next: number;
      chats?: typeof state.sessionChats;
      seeded?: boolean;
      data?: typeof state.pluginData;
    } | null;
    if (kept) {
      Object.assign(state, { sessions: kept.sessions, open: kept.open, nextSession: kept.next });
      Object.assign(state, { sessionChats: kept.chats ?? {}, seededHidden: Boolean(kept.seeded), pluginData: kept.data ?? {} });
    }
  } catch {
    // Nothing kept: no session.
  }

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
      case "plugin:barcode-scanner|check_permissions":
        return { camera: cameraAsked ? scanCamera().after : scanCamera().before };
      case "plugin:barcode-scanner|request_permissions":
        cameraAsked = true;
        return { camera: scanCamera().answer };
      case "plugin:barcode-scanner|open_app_settings":
        return undefined;
      case "plugin:barcode-scanner|scan":
        return new Promise((resolve, reject) => {
          scanning = { resolve, reject };
        });
      case "plugin:barcode-scanner|cancel":
        scanning?.reject("cancelled");
        scanning = null;
        return undefined;
      case "plugin:app|register_listener":
        if (a.event === "back-button") back = { channel: Number((args?.handler as { id: number }).id), index: 0 };
        return undefined;
      case "plugin:app|remove_listener":
        // As Tauri does: only the listener named goes (a newer one may already be in its place).
        if (a.event === "back-button" && back && Number(a.channelId) === back.channel) back = null;
        return undefined;
      case "plugin:app|version":
        return "1.0.0-e2e";
      case "core_me":
        return { id: String((window as unknown as Record<string, unknown>).__ftFakeMeId ?? "ft_me"), name: "Me", mailbox: true, receipts: true, freeUntil: Date.now() + 1e10, autoDownload: state.autoDownload };
      case "core_conversations": {
        const name = (window as unknown as Record<string, unknown>).__ftFakeBobName;
        const lastText = (window as unknown as Record<string, unknown>).__ftFakeBobLast;
        const all = flag("__ftFakeCarol") ? [...state.conversations, conversation("ft_carol12345678", "Carol", "hi!")] : state.conversations;
        return all.map((one) =>
          one.id !== "ft_bob123456789"
            ? one
            : {
                ...one,
                ...(typeof name === "string" ? { name } : {}),
                ...(typeof lastText === "string" ? { last: { ...one.last, text: lastText } } : {}),
              },
        );
      }
      case "core_plugin_open_chat":
        return flag("__ftFakeCarol") ? { contact: "ft_carol12345678", message: "" } : null;
      case "core_requests":
        return state.requests;
      case "core_sessions":
        return state.open.map(sessionView);
      case "core_messages": {
        const older = Number((window as unknown as Record<string, unknown>).__ftFakeLongChat ?? 0);
        const filler = Array.from({ length: String(a.contact) === "ft_bob123456789" ? older : 0 }, (_, at) => ({
          id: `old${at}`,
          outgoing: at % 2 === 0,
          text: `older message ${at}`,
          sentAt: Date.now() - 3_600_000 + at * 1000,
          state: "read",
        }));
        const said = String(a.contact) === "ft_bob123456789" ? (((window as unknown as Record<string, unknown>).__ftFakeBobSays as string[]) ?? []) : [];
        const extra = said.map((text, at) => ({ id: `said${at}`, outgoing: false, text, sentAt: Date.now() - 1000 + at, state: "delivered" }));
        const mine = String(a.contact) === "ft_bob123456789" ? (((window as unknown as Record<string, unknown>).__ftFakeISaid as string[]) ?? []) : [];
        const sent = mine.map((text, at) => ({ id: `mine${at}`, outgoing: true, text, sentAt: Date.now() - 500 + at, state: "read" }));
        type FakeFile = { outgoing: boolean; name: string; mime: string; state: string };
        const files = String(a.contact) === "ft_bob123456789" ? (((window as unknown as Record<string, unknown>).__ftFakeFiles as FakeFile[]) ?? []) : [];
        const shared = files.map((one, at) => ({
          id: `file${at}`,
          outgoing: one.outgoing,
          text: one.name,
          sentAt: Date.now() - 100 + at,
          state: "delivered",
          file: { name: one.name, size: 3_000, mime: one.mime, progress: 1, state: one.state, path: `/x/${one.name}` },
        }));
        return [...filler, ...(state.messages[String(a.contact)] ?? []), ...extra, ...sent, ...shared];
      }
      case "core_plugins": {
        grantLive();
        const many = Number((window as unknown as Record<string, unknown>).__ftFakeManyPlugins ?? 0);
        const extra = Array.from({ length: many }, (_, at) => String(at).padStart(2, "0")).flatMap((n) => [
          { id: `com.example.tool${n}`, name: `Tool ${n} with a rather long name to see it cut`, version: "1.0.0", asks: { network: [], messages: false, send: "nothing" }, granted: { network: [], messages: false, send: "nothing" }, installedAt: 1 },
          { id: `com.flickertalk.game.many${n}`, name: `Game ${n}`, version: "1.0.0", kind: "game", asks: { network: [], messages: false, send: "propose", live: true }, granted: { network: [], messages: false, send: "nothing", live: false }, installedAt: 1 },
        ]);
        if (extra.length) return JSON.parse(JSON.stringify([...state.plugins, ...extra]));
        // A copy, as the real bridge hands over: what the app keeps is never the core's own list.
        return JSON.parse(JSON.stringify(state.plugins));
      }
      case "core_catalogue":
        return catalogue().map((one) => ({ ...one, installed: state.plugins.some((p) => p.id === one.id) }));
      case "core_plugin_add": {
        if (flag("__ftFakeInstallFails")) throw new Error("the catalogue could not be reached");
        const entry = catalogue().find((one) => one.id === a.plugin);
        if (!entry) throw new Error("that tool is not offered here");
        if (!state.plugins.some((p) => p.id === entry.id)) {
          state.plugins.push({
            id: entry.id,
            name: entry.name,
            version: entry.version,
            kind: entry.kind,
            asks: { network: [], messages: false, send: "propose", live: true },
            granted: { network: [], messages: false, send: "nothing", live: false },
            installedAt: Date.now(),
          });
        }
        return undefined;
      }
      case "core_plugin_grant": {
        const plugin = state.plugins.find((p) => p.id === a.plugin);
        // As JSON, as the real bridge carries it: never the app's own (reactive) object.
        if (plugin) plugin.granted = JSON.parse(JSON.stringify(args?.granted ?? {}));
        return undefined;
      }
      case "core_plugin_remove":
        state.plugins = state.plugins.filter((p) => p.id !== a.plugin);
        return undefined;
      case "core_calls":
        return [];
      case "core_card":
        return "https://flickertalk.com/add#card";
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
              outgoing: state.nativeOutgoing,
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
        state.nativeOutgoing = true;
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
      case "core_call_answer_native":
        // The app's own answer button: the core answers, as for the phone's own call screen.
        if (a.call === NATIVE_CALL) answerIncoming();
        return undefined;
      case "core_set_call_routing":
      case "core_call_video_layout":
      case "core_call_mute":
      case "core_call_speaker":
        return undefined;
      case "core_quiet_hours":
        return null;
      case "core_plan":
        return { state: "trial", until: Date.now() + 1e10, age: "unknown" };
      // Like the core (A3): every PIN opens its session or a new empty one, up to seven; an
      // empty one goes when it is closed. Only the first session of PIN 777777 holds a contact.
      // An open one stays open across a reload (2026-10-01), as the core keeps it on disk.
      case "core_session_open": {
        const pin = String(a.pin);
        let found = Object.values(state.sessions).find((s) => s.pin === pin);
        if (!found) {
          if (Object.keys(state.sessions).length >= 7) return null;
          found = { id: `s${state.nextSession++}`, pin };
          state.sessions[found.id] = found;
          if (pin === "777777" && !state.seededHidden) {
            state.seededHidden = true;
            state.sessionChats[found.id] = [conversation("ft_hidden1234567", "Ana", "the secret plan")];
          }
        }
        if (!state.open.includes(found.id)) state.open.push(found.id);
        keepSessions();
        return sessionView(found.id);
      }
      case "core_session_close":
        if (!state.sessionChats[String(a.session)]?.length) forgetSession(String(a.session));
        state.open = state.open.filter((id) => id !== a.session);
        keepSessions();
        return undefined;
      case "core_session_remove":
        forgetSession(String(a.session));
        state.open = state.open.filter((id) => id !== a.session);
        keepSessions();
        return undefined;
      // The id of a chat for a plugin (2026-10-02): deterministic, 43 characters of base64url,
      // its own for each plugin and contact, and only for a contact the user can reach here.
      case "core_plugin_chat": {
        const contact = String(a.contact);
        const here = [...state.conversations, ...state.open.flatMap((id) => state.sessionChats[id] ?? [])];
        if (!here.some((one) => one.id === contact && !one.blocked)) throw new Error("that is not a contact of yours");
        const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let hash = 2166136261;
        let chat = "";
        for (let round = 0; chat.length < 43; round++) {
          for (const c of `${round}|${String(a.plugin)}|${contact}`) hash = Math.imul(hash ^ c.charCodeAt(0), 16777619) >>> 0;
          chat += alphabet[hash & 63];
        }
        return chat;
      }
      // What plugins keep (2026-10-01, §108): apart for each place, as the core keeps it.
      case "core_plugin_record_get":
        return state.pluginData[dataKey("record", a, a.key)] ?? null;
      case "core_plugin_record_set":
        state.pluginData[dataKey("record", a, a.key)] = String(a.value);
        keepSessions();
        return undefined;
      case "core_plugin_record_forget":
        delete state.pluginData[dataKey("record", a, a.key)];
        keepSessions();
        return undefined;
      case "core_plugin_record_keys":
        return dataKeys("record", a, String(a.prefix ?? ""));
      case "core_plugin_record_usage": {
        const used = dataKeys("record", a).reduce((sum, key) => sum + atob(state.pluginData[dataKey("record", a, key)]).length, 0);
        return [used, 4 * 1024 * 1024];
      }
      case "core_plugin_read":
        return state.pluginData[dataKey("memory", a, a.key)] ?? null;
      case "core_plugin_write":
        state.pluginData[dataKey("memory", a, a.key)] = String(a.value);
        keepSessions();
        return undefined;
      case "core_plugin_forget":
        delete state.pluginData[dataKey("memory", a, a.key)];
        keepSessions();
        return undefined;
      case "core_remind_set":
        state.pluginData[dataKey("reminder", a, a.id)] = JSON.stringify({ plugin: a.plugin, id: a.id, at: a.at, text: a.text });
        keepSessions();
        return undefined;
      case "core_remind_cancel": {
        const key = dataKey("reminder", a, a.id);
        const had = key in state.pluginData;
        delete state.pluginData[key];
        keepSessions();
        return had;
      }
      case "core_remind_list":
        return dataKeys("reminder", a).map((id) => JSON.parse(state.pluginData[dataKey("reminder", a, id)]));
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
      case "core_plugin_live_send": {
        grantLive();
        const plugin = state.plugins.find((p) => p.id === a.plugin);
        if (!plugin?.granted.live) throw new Error("that plugin may not go live");
        return true;
      }
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

  /**
   * The incoming call connects, with a video line both ways and both cameras off. Building an
   * answer takes the real core seconds (ICE): the fake takes long enough for any screen shown
   * meanwhile to be seen.
   */
  const connectIncoming = () => {
    setTimeout(() => {
      state.nativePhase = "active";
      state.connectedAt = Date.now();
      callEvent({ kind: "connected" });
    }, 300);
    setTimeout(() => {
      state.video.available = true;
      callEvent({ kind: "video", ...state.video });
    }, 310);
  };

  /** The core answers the ringing call, whoever asked: `answering` at once, then it connects. */
  const answerIncoming = () => {
    if (state.nativePhase !== "ringing") return;
    state.nativePhase = "connecting";
    setTimeout(() => callEvent({ kind: "answering" }), 1);
    connectIncoming();
  };

  /** The other phone calls this one; `answered`: this phone's own screen answered before the offer. */
  const ring = ({ video = false, answered = false }: { video?: boolean; answered?: boolean } = {}) => {
    state.nativeContact = "ft_bob123456789";
    state.nativeOutgoing = false;
    state.nativePhase = answered ? "connecting" : "ringing";
    Object.assign(state.video, { available: false, camera: false, paused: false, facing: "front", remote: false, remotePaused: false });
    callEvent({ kind: "incoming", video, sdp: "their-offer", ...(answered ? { answered: true } : {}) });
    if (answered) connectIncoming();
  };

  const fake = {
    calls,
    state,
    emit,
    ring,
    phoneAnswers: answerIncoming,
    scanned: (content: string) => {
      scanning?.resolve({ content, format: "QR_CODE", bounds: null });
      scanning = null;
    },
    /** Whether the app listens to the back button now, without pressing it. */
    listening: () => back !== null,
    back: () => {
      if (!back) return false;
      const channel = handlers.get(back.channel) as unknown as ((raw: { message: unknown; index: number }) => void) | undefined;
      channel?.({ message: { canGoBack: true }, index: back.index++ });
      return true;
    },
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
