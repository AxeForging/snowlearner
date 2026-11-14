# 003 — Audio devices, voices, pluggable TTS and learner-friendly listening

Status: approved in chat (2026-09-27).

## Problem
- No way to choose/test the microphone or speakers, or to pick/test voices.
- OS voices are robotic; people running local neural TTS (Kokoro, Piper) can't use it.
- Listening is tuned for fluent speakers: 4 s to start, cut after 0.9 s of silence,
  fixed 7 s cap — learners need thinking time and hesitate mid-sentence.

## Listening (endpointer v2)
- **Echo guard**: ignore the first 250 ms after the mic opens (tail of the TTS voice).
- **Thinking time** before you start: repeat 6 s, recall 10 s (countdown shown).
- **DC removal** (2026-09-27): a 30 Hz high-pass runs on the mono mic signal before the
  endpointer and whisper. A laptop mic measured +0.044 DC on one channel (room noise ~0.0013);
  counted as noise, it raised the gate above a normal voice.
- **Calibration** (revised 2026-09-27): the floor is the 20th percentile of the last 6 s
  (guard included) and is frozen once speech starts; onset = 3 loud frames within 5; a
  word-sized burst (≤ 1.5 s) already going when the mic opens counts once the quiet after it
  shows the real floor; a word begun as thinking time ends gets 0.5 s of grace.
- *(original)* **Calibration** from the quietest frames (not the first ones), so starting to talk
  instantly doesn't make it deaf; the noise floor keeps adapting while you're quiet.
- **Adaptive pause**: expected duration ≈ 0.45 s/word + 0.6 s. Until you've spoken
  ~60% of that, a pause may last 1.8 s ("I'm… hungry"); after, 0.8 s ends the turn.
- **Cap counted from speech start**: expected × 2.5 + 2 s (4–20 s).
- **Manual**: the practice hotkey / orb click while listening = "done".
- **Visible**: live level meter + "speaking" indicator in the caption.
- **Whisper junk filter**: transcripts that are only a known hallucination
  ("Thank you for watching", "Legendas pela comunidade Amara.org"…) count as silence.

## Audio devices
- `mic` / `speaker` settings (empty = system default), chosen by name.
- `snowlearner audio mics|speakers`, `snowlearner audio test-mic` (live meter, then
  shows what the recognizer heard).
- Panel "Áudio" tab: microphone, test mic (meter + transcript), speaker.

## Voices & engines (`tts_engine`)
| engine | how | voices listed from |
|---|---|---|
| `system` (default) | spd-say / espeak-ng / say / SAPI | the engine's own list |
| `http` | OpenAI-compatible `POST {tts_url}/audio/speech` → WAV, played by the app | `GET {tts_url}/audio/voices` (Kokoro-FastAPI) |
| `command` | `tts_command` template (`{text}` `{lang}` `{voice}` `{out}`), run without a shell; if `{out}` is used the app plays the WAV | — |

Kokoro defaults when the voice is empty: pt-BR `pf_dora`, en `af_heart`, es `ef_dora`.
`snowlearner voices list [--lang]`, `snowlearner voices test [--lang] [--voice]`;
panel: engine, native voice, target voice, test voices.

## Docs
Animated WebP clips in the README rendered from `snowlearner snapshot --scenario … --frames`.

## Tests
Endpointer (echo guard, think time per mode, hesitation kept, cap from speech start,
instant talker, manual finish), hallucination filter, voice-list parsers (spd-say,
espeak-ng, Kokoro JSON + language filter), command template (no shell, `{out}`),
HTTP engine against a local fake server, player resampling/channel mapping, settings
validation, panel tabs, lesson think-time/manual stop.

## Added during the round
- **TTS as a trait**: `SpeechEngine` (describe / available / voices / speak → played or WAV);
  engines register in `Voice::new`; playback and pickers are shared.
- **Shortcut cheat sheet** next to the orb: on hover, or while Ctrl+Alt is held
  (`control/modkeys.rs`: Windows `GetAsyncKeyState`, macOS `CGEventSourceFlagsState`,
  X11 `XQueryKeymap`; Wayland can't — hover only).
- **Magic hand**: hold Ctrl+Alt, or `snowlearner grab` / Ctrl+Alt+G for 15 s. The overlay takes
  clicks only while it's on. Pick up the mage (grumpy lines) or the warrior (shy lines), drag,
  release → they fall into the snow. Frozen warriors can't be lifted; a held mage can't cast.

## Recognition speed (2026-11-13, #58)
A learner on Windows waited about 16 s for every one-word answer. Measured with
`snowlearner model bench` on the same machine (AMD Ryzen 7 2700, `base` model, 6 threads, the
same built-in clip, whisper's full 30 s window):

| build | one pass |
|---|---|
| Windows, v0.1.9 as released | 17.21 s |
| Windows, whisper.cpp compiled with `/O2` | 3.26 s |
| Linux (WSL2) | 3.13 s |

- **Cause: the Windows build of whisper.cpp was not optimized.** With the Visual Studio generator
  the `cmake` crate (0.1.58) passes `CMAKE_C_FLAGS_RELEASE` and `CMAKE_CXX_FLAGS_RELEASE` itself,
  replacing CMake's `/O2 /Ob2 /DNDEBUG` with `-nologo -MD -Brepro -W0`: no compiler line of the
  build had an optimization flag. The release workflow now sets both variables on Windows
  (`-MD -O2 -Ob2 -DNDEBUG`); a source build on Windows must do the same (README).
- **Target:** a one-word answer is transcribed in 4 s or less on that machine with the `base`
  model. `transcribed in N s` in `snowlearner.log` is the real number; `snowlearner model bench`
  gives it without a microphone, and its first two rows are what a lesson does.
- **Threads stay at six.** With the optimized Windows build, giving whisper all 16 threads took
  76.84 s for the pass that takes 3.26 s with six.
- **Tried and rejected: a narrower encoder window.** Whisper encodes its whole 30 s window even
  for one word, and a 7.5 s window (`audio_ctx` 375) cut a pass to 0.67 s. It also made
  recognition worse in the one end-to-end run made with it: "Muchas gracias" was heard as
  "Muchas latias", and with the vocabulary hint "Vale" was accepted as "¡Pare!". Accepting a
  wrong answer costs a learner more than two seconds, so lessons keep the full window. Other
  widths were not tested.
- **Adopted in #72: ggml's `GGML_LLAMAFILE=ON` matrix code**, first measured at 1.88 s (Windows)
  and 2.29 s (Linux) for the same pass. It does not touch the window. `.cargo/config.toml` turns
  it on for every build inside the repository (CI, releases, `make`); `cargo install --git` does
  not read that file (README).

## The hinted second pass (2026-11-14, #72)
When the first pass is not accepted, a lesson runs a second pass with the expected words as a
hint (`Recognizer::transcribe_expecting`), so that answer takes two passes. Each pass is logged
with what it heard, its time and its verdict (`first pass "…" in N s: …`).

In one practice session on v0.1.13 (20 answers, the reporting machine), 15 were accepted on the
first pass in 2.8–3.1 s; the hinted pass ran for 5, turned 3 of them into a pass ("Ten", "Nine",
"Cold", 5.8–9.5 s in all) and changed nothing for 2 (6.1 s). The first pass had lost or changed
the word's first sound ("Ten" → "And", "Cold" → "Hold"). So the hinted pass stays: it rescues
right answers.

Measured with `model bench` on the reporting machine (6 threads per pass, full window):

| build | one pass | two passes at the same time |
|---|---|---|
| v0.1.13 | 3.15–3.46 s | 4.53–4.67 s (3 + 3 threads: 6.13–6.42 s) |
| `GGML_LLAMAFILE=ON` | 1.79–2.14 s | 2.48–2.85 s (3 + 3 threads: 2.97–3.22 s) |

Every 30 s row heard the same text in both builds ("10, please.", "Cold.", "4" from the Windows
Zira voice), and the speech end-to-end tests pass with it.

- **Tried and rejected: running both passes at once on v0.1.13.** They share the CPU, so every
  answer would take about 4.6 s, above the target, against 3.3 s for three answers in four.
- **Adopted: the faster build.** The hinted pass still runs after the first one; with one pass
  at about 1.9 s, an answer that needs it takes about 3.8 s.
- **Not adopted: both passes at once on the faster build** (about 2.5 s for every answer). It
  doubles the CPU spent on every answer to speed up one in four; it is worth its own change only
  if real sessions show hinted answers above 4 s.
