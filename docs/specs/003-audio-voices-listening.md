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
