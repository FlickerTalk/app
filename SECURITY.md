# Security policy

FlickerTalk carries private conversations. If you find a security problem, please tell us
privately.

## How to report

- GitHub: **Report a vulnerability** under this repository's *Security* tab (private
  vulnerability reporting), or
- email: [info@flickertalk.com](mailto:info@flickertalk.com).

Please include the affected version (Settings, Version in the app, or the commit), how to reproduce
the problem, and what an attacker could do with it. Do not include private conversations, private
keys or other people's personal data.

Test only with your own phones. Do not put load on `api.flickertalk.com`: run your own router from
[FlickerTalk/server](https://github.com/FlickerTalk/server).

## What happens next

We are a small team. We aim to confirm that we received your report within a few working days, and
to tell you within about two weeks whether we could reproduce it and what we plan to do. We cannot
promise a date for the fix. We will keep you informed, publish a GitHub security advisory once the
fix is released, and credit you if you want. There is no bug bounty.

## Supported versions

The latest release (Google Play and [GitHub Releases](https://github.com/FlickerTalk/app/releases))
and `main`.

## In scope

- Reading or altering messages, files or calls, or the keys that protect them.
- Recovering a private key, or opening a backup without its recovery phrase.
- Impersonating a phone or a contact, or putting oneself in the middle of a connection.
- A plugin getting more than the user granted it: leaving its frame, or reaching the identity, keys,
  contacts or history; or the app running a package that the signed catalogue does not list.
- Making our server store, log or hand over more than the README and
  [how it works](https://flickertalk.com/how-it-works/) describe, or reaching a phone without its
  routing code.
- Crashing a phone, or making it misbehave, with what someone else sends it.

## Out of scope

- Attacks that need a compromised, rooted or unlocked phone.
- The limits we document on [how it works](https://flickertalk.com/how-it-works/), such as a
  contact seeing your IP address on a direct connection.
- Volume attacks against our servers, problems in Google's or Apple's push services, and scanner
  output without a demonstrated impact.

How the app is built and what it is designed to protect against:
[flickertalk.com/security](https://flickertalk.com/security/).
