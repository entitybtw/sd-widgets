# sd-widgets — notes for agents

A Steam Deck control surface (`sdw-deck`) and a PC companion daemon
(`sdw-daemon`) in one Rust workspace. Start with **`docs/README.md`**, then open
only the page of the task. Open work is in `NEXT.md` (or `next.md`): a local file
of the maintainer, git-ignored and never committed. It is the queue of the work
and its instructions. Read it before starting; on "continue" (продолжай / делай /
дальше), take its first open item without questions, and update it when stopping.
If it is missing (a fresh clone), say so instead of guessing the queue.

## Talking to the maintainer

- Answer in **Russian**. Code, comments, commit messages and docs stay in English.
- Report honestly: what is done, what is not, what was checked and how.
- Never open windows on the maintainer's desktop to "check" the GUI. Build and
  type-check; run a headless check only when it cannot disturb the session.

## Docs

- Docs describe the current state; history lives in git. New knowledge goes to the
  page of the topic, open work to `NEXT.md`, and finished work leaves `NEXT.md`.
  Public docs never link to `NEXT.md` (it is not in the repository).
- **They are public.** Never write credentials, tokens, private host names, IPs,
  e-mail addresses or home paths. Write `$HOME`, `<deck>`, `<computer>`.
- Keep English and Russian READMEs in step. The product name is lowercase:
  `sd-widgets`.

## Rules

| # | Rule |
|---|---|
| R1 | Never `git checkout`/`git stash`/`git reset` over uncommitted work. Back up a file before a risky edit. |
| R2 | Commit or push only when the maintainer asks. Releases are tagged and published deliberately. |
| R3 | Long-running work and scratch files live in `work/` (git-ignored), not in `/tmp`. |
| R4 | The visual language is hard-edged Grid: no corner radius, hairline borders, 32 px rhythm, Material Symbols only. New UI follows it. |
| R5 | Preserve the Quadra attribution in both READMEs. The Tide clock keeps its concept and font but is drawn in the Grid language; do not copy the reference screen literally. |
| R6 | One repo, two binaries. Shared types go to `sdw-core`; never duplicate a serialized type on one side. |
| R7 | Every config field needs a `serde(default)` and a sensible default, so old configs keep loading. |
| R8 | `cargo check --workspace` before saying a change compiles; `cargo fmt` on touched crates. |
| R9 | Remote commands and addon actions are enums/declared ids, never raw shell from deck input. Keep the whitelist honest. |
| R10 | Small steps; one topic per commit. |

## Build and check

```bash
make check                 # cargo check --workspace
make release               # target/release/sdw-deck, sdw-daemon
packaging/linux/build-appimage.sh all
```

Cross builds for Windows and macOS cannot be produced in this environment; do not
pretend they were tested.

## Where things are

```
crates/sdw-core      shared model, addons, API client, auth, discovery, UDP, i18n
crates/sdw-deck      Deck app: app.rs, pages.rs, clock.rs, addons_ui.rs, theme.rs
crates/sdw-daemon    PC daemon: server.rs, state.rs, monitor.rs, gui/, config.rs
assets               fonts and icons
packaging            install.sh, desktop files, systemd unit, AppImage / Windows / macOS scripts
docs                 architecture, API, themes, addons, monitor
```
