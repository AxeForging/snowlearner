# 005 — Start from the basics, see your progress, install in one command

Status: approved in chat (2026-09-27): "começar do bem básico, não frases de cara, e ir
formando frases; um menu de progresso com atalho; install ultra simples; lança a v0.1.0".

## Path (trilha): words → chunks → phrases
- New built-in decks `decks/<lang>.basics.toml`: ~44 first words and ~30 short chunks
  (2–3 words) per language, each still a real situation (topic + pt-BR situation).
- `learn/path.rs` orders the selection by difficulty: stage (1 word → words, 2–3 → chunks,
  else phrases), then CEFR level (unleveled = B1), then length.
- Only a few items are *new* at a time: the lesson picks among what you already know or
  started, plus the next new items in order until `LEARNING_SLOTS` (4) are in progress.
  An item is known after `RECALL_AFTER` (2) successes. Knowing items unlocks the next ones.
- Topic and max level filters still apply before the path.

## Progress (Ctrl+Alt+P, `snowlearner progress`)
- `learn/progress.rs` (pure): per stage and per topic known / learning / total, the
  current stage, and the next items that will unlock.
- Panel gets a third tab, PROGRESSO; the hotkey (`hotkey_progress`, GNOME shortcut too)
  and `snowlearner progress` over IPC open the panel on it.
- `snowlearner progress --print` prints the same in the terminal, app running or not.

## Install and first start
- Release workflow on `v*` tags builds Linux x86_64 (glibc 2.35), macOS arm64 and
  Windows x86_64 archives and attaches them to a GitHub release. CI stays Linux-only.
- `install.sh` (Linux/macOS) and `install.ps1` (Windows): download the latest release,
  put the binary on the user PATH, then run `snowlearner setup`.
- `snowlearner setup [--lang en|es] [--no-model]`: idempotent — writes the config,
  downloads the speech model, registers GNOME shortcuts where available, adds an app
  launcher (Linux .desktop), and prints how to start. Safe to run again.

## Test plan
- Path: stage classification, ordering, unlock window, known items stay, missed items
  stay in, filters respected, empty selection.
- Progress: counts per stage/topic, current stage, next-up list, empty history.
- Menu: progress tab renders with supported glyphs, tab cycling includes it.
- CLI (real binary): `progress --print` on empty and practiced history; `setup --no-model`
  writes config once and is idempotent; IPC `progress` reaches a running instance.
- install.sh: shellcheck-clean; exercised against the real v0.1.0 release after tagging.
