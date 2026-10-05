# Working rules for assistants in this repository

- **Authorship.** Commits, pull requests and anything pushed to git carry only the human author
  (ioanbeilic). Never add an assistant as co-author, collaborator or session link: no
  `Co-Authored-By`, no `Claude-Session`, no "Generated with" lines, in commit messages, PR bodies,
  code comments or docs.
- **One repo per plugin.** Every plugin lives in its own repository (`FlickerTalk/plugin-<name>`),
  with its own licence and tests; the games live together in `FlickerTalk/games`. This repository
  only carries the seed (`src-tauri/resources/plugins`) and the catalogue client; the rest is
  downloaded on install. Implement plugins and games one at a time, each in its repository.
- **Both platforms.** Every feature of the app ships for Android and iOS at once.
- **Licences.** Only MIT, BSD, Apache-2.0, ISC or 0BSD code in games and plugins; check word lists,
  piece sets, icons and fonts separately, and use generic game names, never trademarks.
