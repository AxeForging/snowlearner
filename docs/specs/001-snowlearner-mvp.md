# 001 — Snowlearner MVP

Status: approved by user in chat (2026-09-27), iterating.

## Problem
Practicing speaking a new language needs small, frequent nudges. A pixel-art frost
mage lives on the desktop, slowly freezing the screen. Saying phrases in the
language you are learning melts the ice. At the end of the day it recaps what you
practiced, out loud.

## Users
A pt-BR speaker learning English (first) or Spanish. Decks are data, so other
pairs work too.

## Requirements
1. **Mage** — pixel-art mage walks along the bottom of the screen and throws ice
   cubes that shatter and pile up as snow.
2. **Summons** — every so often the mage summons a friend (snowman, penguin) that
   bursts frost onto the screen edges.
3. **Warrior** — a small warrior tries to survive the freeze: wanders, builds
   *fogueiras* (campfires) that slowly melt nearby snow, and has a warmth meter that
   drops as the screen freezes. He gives pt-BR tips (hotkey, hint for the next phrase).
   At zero warmth he becomes an ice statue until you get a phrase right; every success
   gives him a warm burst.
4. **Commitment level** (*compromisso*) — `chill | steady | committed | relentless`
   scales snow rate, summon interval and how often the mage asks you to practice.
5. **Lessons** — phrases have a pt-BR cue with the target part marked:
   `Para dizer que estou com fome, devo dizer: {{I'm hungry}}`.
   - TTS reads native segments with the native voice, `{{…}}` with the target voice.
   - Captions show the cue; target text highlighted; the segment being spoken lit up.
6. **Speaking** — hotkey (or `snowlearner say`) starts a challenge: cue is spoken,
   mic listens, local STT transcribes in the target language, and the best-matching
   window of what was heard is scored (so mixed pt/en answers still pass).
   Per-word green/red feedback. Success melts snow and frost.
7. **End of day** — at `summary_time` (or hotkey / `snowlearner summary`) a panel lists
   the phrases practiced today and TTS reads them back.
8. **No cloud, no LLM** — OS-native TTS; STT is local whisper.cpp (Linux has no OS
   STT). Without the `stt` feature, phrases are self-confirmed with the hotkey.

## Cross-platform design
| Concern | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Window (default `auto`) | overlay | overlay | overlay | window (forced X11 overlay with `--mode overlay`) |
| Transparency | if surface supports alpha, else window mode | yes | yes (compositor) | via XWayland |
| Global hotkey | `global-hotkey` | `global-hotkey` | `global-hotkey` | bind a system shortcut to `snowlearner say` (IPC) |
| TTS | PowerShell System.Speech (SAPI) | `say` | `spd-say` → `espeak-ng` | same |
| STT | whisper.cpp | whisper.cpp | whisper.cpp | whisper.cpp |

`snowlearner doctor` reports what works on the current machine and how to fix the rest.

## Design
- Render: CPU pixel canvas (virtual resolution = window / pixel scale) uploaded to a
  wgpu texture, nearest-neighbour upscale; alpha mode picked from surface caps.
- Scene is a pure, seeded simulation (`step(dt)` + `draw(canvas)`) — testable headless,
  and `snowlearner snapshot out.png` renders frames without a window.
- Speech runs on a worker thread; results come back as winit user events.
- IPC: localhost TCP (`127.0.0.1:<ipc_port>`), line protocol `challenge|summary|quit`.
- Storage: SQLite (`attempts` table) in the platform data dir; `SNOWLEARNER_HOME`
  overrides all paths (tests, portable installs).

## Test plan
- Unit: matcher (exact, punctuation/case, missing accents, mixed-language window,
  unrelated, empty), cue parser, endpointer, resampler, snow/frost melt & caps,
  commitment levels, phrase picker, deck + config validation, font coverage, store
  aggregation, deterministic scene run.
- Binary (built `snowlearner`): `--help`, `decks`, `config init` (no clobber),
  `summary` empty + seeded DB, `say` with/without a listener, `snapshot` writes a PNG.
- Whisper end-to-end test is `#[ignore]`d unless a model is present.

## Out of scope (v1)
Native macOS/Windows STT backends, CJK fonts, spaced repetition beyond
"least-practiced first", tray icon.
