# Contributing

Thank you for wanting to help. Issues and pull requests are welcome. For anything bigger than a
fix, please open an issue first, so we can agree on the approach before you write the code.

Security problems never go in a public issue or pull request: see [SECURITY.md](SECURITY.md).

## Build and test

The README explains how to build. Before you open a pull request, run the checks CI runs on it:

```sh
npm ci
npm run typecheck
npm test
npm run test:e2e   # once before: npx playwright install chromium
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI also builds the Android app and an iOS build for the simulator. If you touched the Android or
iOS bridge, run its own tests too (below).

## The test comes first

We work test first: write the test that describes the change, see it fail for the right reason,
then write the code that makes it pass, then tidy up. A bug fix starts with a test that reproduces
the bug.

- **UI:** Vitest, next to the module (`src/foo.ts`, `src/foo.test.ts`). End-to-end: Playwright in
  `e2e/`, the real UI against a fake core (`e2e/fake-core.ts`).
- **Rust:** unit tests in a `#[cfg(test)] mod tests` in the same file; integration tests in each
  crate's `tests/`. Tests that need the real network are marked `#[ignore]`; most of them run
  every night.
- **Android bridge (Kotlin):** tests in `src-tauri/platform/android/src/test`. After one
  `npm run tauri android build` or `dev`, run
  `./gradlew :tauri-plugin-ft-platform:testDebugUnitTest` from `src-tauri/gen/android`.
- **iOS bridge (Swift):** tests in `src-tauri/platform/ios/Tests`. After one
  `npm run tauri ios build` or `dev` on a Mac, run
  `xcodebuild test -scheme tauri-plugin-ft-platform -destination 'platform=iOS Simulator,name=<simulator>'`
  from `src-tauri/platform/ios`. CI does not run these yet.

Business logic belongs in the Rust crates (`crates/ft-*`), where it can be tested without Tauri.
`src/` and `src-tauri/` stay thin.

## Commits and pull requests

- Commits: in English, in the imperative ("Fix…", "Add…"), one change per commit, subject line
  of 72 characters at most.
- Pull requests: small. Title and description in English, saying what changes, why, and how you
  tested it. CI must be green.
- Code, identifiers, comments, log messages and test names are in English.

Comments in the code sometimes cite sections of our internal design notes as "Plan §N" or "§N".
Those notes are not public; what a contributor needs is in the README, in this file and in the
code.

## What we will not merge

- **A new dependency without a reason.** Say in the pull request what it does, why existing code
  cannot do it, its licence and what it adds to the app's size.
- **A wire-format change that older apps cannot read.** `Packet`, `Signal` and `Envelope` in
  [`crates/ft-protocol`](crates/ft-protocol/src/lib.rs) are versioned CBOR. Add new packet types or
  optional fields: an older app decodes an unknown type as `Body::Unknown` and ignores it. Anything
  an older app would misread needs a version bump and a test.
- **Anything that makes our server store or log more**, or makes the app send it more. The README
  and [how it works](https://flickertalk.com/how-it-works/) describe what the server keeps. Changes
  to the server go to [FlickerTalk/server](https://github.com/FlickerTalk/server) and need a strong
  reason.
- **Visible text written into the code.** Every string goes through i18n: add the key to
  `src/i18n/en.json` and its translation to the other 20 catalogues in `src/i18n/` (the i18n test
  fails otherwise). Native notifications also go in the 21 `ft_strings.xml` files (Android) and
  the `Localizable.strings` files (iOS). If you cannot translate a string, say so in the pull
  request and we will add the translations before merging.
- **Code, identifiers or comments in a language other than English.**
- **Analytics, crash reporting or tracking of any kind**, and **cryptography of our own**: we use
  vodozemac, `crypto_box`, BLAKE3 and Ed25519 as they are.

## Licence

By contributing, you agree that your contribution is licensed under the
[AGPL-3.0](LICENSE), like the rest of this repository. The FlickerTalk name and logo are not
covered by it (see the README).
