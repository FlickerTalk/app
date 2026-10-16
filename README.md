# FlickerTalk

FlickerTalk is a private messenger: texts, photos and files, voice messages, and voice and video
calls. No account. No phone number required. No activity logs. Your chat history stays on your
phone, not on our servers. [flickertalk.com](https://flickertalk.com)

- **Android:** [Google Play](https://play.google.com/store/apps/details?id=com.flickertalk.app)
- **iPhone:** coming soon on the App Store.
- **Video:** [the trailer](https://youtu.be/QNMZLA05s7c) (1 min) and
  [the full manual](https://youtu.be/rQNRywOx_P8) (24 min).

<p>
  <img src="docs/readme/chat.png" width="240" alt="A chat with a video, a PDF, two voice messages and a quoted reply, all delivered">
  <img src="docs/readme/chats.png" width="240" alt="The chat list: a circle, two contacts and, below them, a session with its own contact">
  <img src="docs/readme/apps.png" width="240" alt="The Apps tab: the tools in a grid">
</p>

## What it does

- **No account:** no phone number, no email, no password. Your identity is a key made on your
  phone; you add people by scanning their code or opening their link. We have no directory of
  users and never see your address book.
- **Phone to phone:** messages travel directly between phones when possible, over an encrypted
  WebRTC connection, end-to-end encrypted with Olm (vodozemac). When a direct path is impossible,
  our relay carries the same encrypted traffic.
- **Optional mailbox:** if the other phone is offline, an optional mailbox holds your message
  end-to-end encrypted. It is deleted when picked up, or after 7 days at the latest. We cannot
  read it. If either of you turns the mailbox off, the message waits on the sender's phone until a
  direct connection is possible.
- **Files and calls** are one to one and never go through the mailbox. During a call you can
  present a whiteboard or a PDF, page by page.
- **Circles:** a small, closed group of your contacts, up to 32 members, for text messages. Our
  server keeps no record of circles.
- **Sessions with a PIN:** a separate space, with its own contacts and PIN.
- **Per-contact rules:** mute, accept only messages or only calls, turn receipts off, keep history
  for a while or delete after reading; weekly quiet hours.
- **Tools and games:** 17 tools and 13 games are in the Apps tab, from a signed catalogue: install
  only the ones you want. Each one runs in isolation and gets only what you allow it, never your
  chat history, your contacts or your keys.
- No advertising. No analytics. No tracking profiles.

The app is free to use; the tools and extra PIN-protected sessions are an optional Premium
subscription, with a 15-day trial.

## What our server knows, and what it does not

Our server is a small router ([FlickerTalk/server](https://github.com/FlickerTalk/server)). It is
not a chat server: it wakes phones, passes connection offers between them, and holds messages that
could not be delivered yet, end-to-end encrypted. For each phone it keeps:

| Data | Why | For how long |
| --- | --- | --- |
| Device ID (`ft_` + base58 of the BLAKE3 hash of the public key) and the Ed25519 public key | To check that a request is signed by that phone | Until you erase the phone in the app |
| BLAKE3 hashes of the phone's eight routing codes, one per space: the main list and up to seven sessions with a PIN. All eight are registered, used or not | Only someone with a routing code, which travels in your contact card, can wake you or leave you a message | Same |
| Push provider (Google or Apple) and push token, encrypted with a key kept outside the database | To wake the phone | Same, or until Google or Apple report the token invalid |
| Eight bits (`silent_slots`), one per space, marking the sessions you have left | So that a session you have left gets no wake-ups or calls. We can see when a bit changes, not who is in a session | Replaced at every registration |
| Time of the last registration or push token change (`updated_at`) | Written with the record; nothing uses it yet | Same as the device ID |
| Messages that could not be delivered: end-to-end encrypted, sealed again for the recipient, without sender. Each has an ID that contains the time it was left, an expiry time and the number (0–7) of the space it came through | To deliver them later | Until the recipient's phone confirms it has them, at most 7 days |

The mailbox is an `UNLOGGED` PostgreSQL table: it never reaches the write-ahead log or a backup.
The device records are in our database backups, which are encrypted before they leave the server.

Held in memory only, never written to disk:

- connection offers and answers for a phone that is not connected, end-to-end encrypted: at most
  55 seconds;
- which phones are connected right now;
- the nonces of recent signed requests, for a few minutes, to refuse replays;
- rate-limit counters, for one minute, per device, per recipient and per connection address
  hashed with a salt made at start;
- how many suggestions each device sent in the last 24 hours.

Nothing else: no contacts or address books, no circles or their members, no conversations, no call
history, no names, phone numbers or email addresses, no access logs and no history of IP addresses.
The whole API is in
[`ft-router/src/v1.rs`](https://github.com/FlickerTalk/server/blob/main/ft-router/src/v1.rs):
register, erase, set or clear the push token, connect (WebSocket), signal, leave a message, collect
and acknowledge messages, get TURN credentials, and send a suggestion (forwarded by email, not
stored). There is no endpoint for messages, conversations, users, profiles, history or attachments.

While a request or a connection is open, the server sees its IP address and, if it is signed,
which phone made it. Mailbox messages and connection offers are not signed by the sender: they
need only the recipient's routing code.

## What it does not protect

- During a direct connection, the other phone can see your public IP address. In Settings you can
  make calls always go through our relay, which hides it from the other person during calls.
- Google or Apple deliver the wake-up notification, so they see that your phone got one and when.
  The notification never contains a message or who sent it.
- No app can protect you from a phone that is compromised or unlocked in someone else's hands,
  from malware, from screenshots, or from the people you write to copying what you send.
- FlickerTalk itself has not been audited. The encryption library it uses, vodozemac, has.

Who can see what, party by party: [how it works](https://flickertalk.com/how-it-works/).

## This repository

The whole client: the Tauri 2 app (Vue 3 + Ionic in `src/`, Rust in `src-tauri/`) and the Rust
core in `crates/ft-*`, which holds the business logic. Android and iOS are the target platforms;
desktop builds but is not published.

```sh
npm install
npm test            # frontend tests (Vitest)
npm run test:e2e    # end-to-end tests of the UI (Playwright; `npx playwright install chromium` once)
npm run typecheck
cargo test --workspace   # Rust tests, no network
npm run tauri android dev
```

iOS release builds with Xcode 27 also need `rustup component add llvm-tools`: the patched
`swift-rs` in `Cargo.toml` uses its `llvm-objcopy` to export the Swift entry points that Xcode 27
leaves local (see the comment there).

Google Drive sign-in needs the app's OAuth client ids at build time: `FT_GOOGLE_CLIENT_ID`
(Android) and `FT_GOOGLE_IOS_CLIENT_ID` (iOS), both `<id>.apps.googleusercontent.com`. A release
build for Android or iOS stops without its platform's id (`src-tauri/build.rs`); a debug build only
warns and cannot sign in. A release that is never published can build without it with
`FT_ALLOW_NO_GOOGLE_CLIENT=1`. `tauri ios build` only passes `TAURI*` variables to the Xcode build,
so for iOS export the id as `TAURI_FT_GOOGLE_IOS_CLIENT_ID` (also read; the plain name wins).

Builds are signed and published by GitHub Actions: every push to `main` produces a `canary`
prerelease, and every `vX.Y.Z` tag a release.

## Licence

[AGPL-3.0](LICENSE). The server is in [FlickerTalk/server](https://github.com/FlickerTalk/server).
Security issues: see [SECURITY.md](SECURITY.md).

The FlickerTalk name and logo are not licensed under the AGPL (section 7(e)): forks are welcome,
but must use their own name and icon. The public router at `api.flickertalk.com` serves the
official FlickerTalk apps only; a fork should run its own router from
[FlickerTalk/server](https://github.com/FlickerTalk/server).
