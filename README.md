# FlickerTalk

Private messages, phone to phone. Text, files, voice and video calls between two people, with the
history living only on the phones — never on a server. [flickertalk.com](https://flickertalk.com)

- **No account:** no phone number, no email, no password. Your identity is a key made on your
  phone; you add people by scanning their code or opening their link.
- **Phone to phone:** messages travel over an encrypted WebRTC connection straight to the other
  phone, end-to-end encrypted with Olm (vodozemac).
- **Delivered, not stored:** when the other phone is off, the message waits in a mailbox on our
  server, encrypted so that we cannot read it, until it is picked up or 7 days pass. Either side
  can turn the mailbox off.
- **Files and calls** are one to one and never go through the mailbox.

What our server keeps, who can see what, and the limits of the model: see
[how it works](https://flickertalk.com/how-it-works/).

## This repository

The whole client: the Tauri 2 app (Vue 3 + Ionic in `src/`, Rust in `src-tauri/`) and the Rust
core in `crates/ft-*`, which holds the business logic. Android and iOS are the target platforms;
desktop builds but is not published.

```sh
npm install
npm test            # frontend tests (Vitest)
npm run typecheck
cargo test --workspace   # Rust tests, no network
npm run tauri android dev
```

Builds are signed and published by GitHub Actions: every push to `main` produces a `canary`
prerelease, and every `vX.Y.Z` tag a release.

## Licence

[AGPL-3.0](LICENSE). The server is in [FlickerTalk/server](https://github.com/FlickerTalk/server).
Security issues: see [SECURITY.md](SECURITY.md).
