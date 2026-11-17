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
- **Silence** (`Lesson::silence`, #98): nothing heard, or only a known hallucination, is never an
  attempt. The first silence of a question opens the mic again at once ("Hã? Fala de novo!"):
  the learner may just have been slow to start. A second silence in a row shows *NÃO OUVI NADA*
  (`Status::Silent`, not the wrong-answer header) and a footer counting down to closing
  ("… · fecha em 25 s"); the hotkey or the orb asks again, with a fresh second chance. So a
  learner who walked away is asked twice at most, never in a loop (the reason for 9352f02).
  The footer names the hotkey only when it reaches the app here (a global hotkey that registered,
  or the GNOME shortcut); otherwise only the orb. A long footer wraps inside the caption.
  Before #98 the first silence already waited 25 s under "QUASE! TENTE DE NOVO" with no sign of
  what came next, and a learner on Linux read it as frozen.

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

## Recognition on a server (`stt_engine`, #59)
| engine | request | reply |
|---|---|---|
| `local` (default) | whisper.cpp in the app | — |
| `openai` | `POST {stt_url}/audio/transcriptions`, multipart: `file` (WAV), `language`, `response_format=json`, `model` when `stt_model` is set, `prompt` on the hinted pass | `{"text": …}` |
| `asr` | `POST {stt_url}/asr?language=…&output=json[&initial_prompt=…]`, multipart: `audio_file` (WAV) | `{"text": …, "segments": …}` |

Both shapes were read from the reporter's servers' own OpenAPI (Whisper Asr Webservice 1.9.1):
`192.168.15.4:9000` exposes only `/asr`, `192.168.15.6:9000` both. The WAV is the answer as
whisper.cpp would get it (`transcriber::prepare`: 0.3 s lead-in, at least 1.1 s), 16-bit mono 16 kHz.

- **Opt-in.** The default is `local`; with it no request is ever made. Otherwise the answer's audio
  goes to `stt_url` only, and the README says so.
- **Same lesson logic.** `transcriber::Transcriber` holds the two passes (the hinted one only when
  the first missed) and their log lines for both recognizers; the server is one more `pass`.
- **Timeouts.** 3 s to connect, 30 s per answer: a server loading `large-v3-turbo` for its first
  request took 18 s. A failure is a `SpeechEvent::Failed` → self-confirmation, reason in the log.
- **Measured** with a synthetic clip ("Me too, I love pizza.", Windows SAPI voice), 2026-11-17:
  `/asr` 0.6–0.9 s on .4 and 0.4 s on .6; `/v1` with `base` 0.2 s on .6. Local `base` is about
  1.9 s per pass on the same machine.
- **Known quirk.** On `/v1` with model `base`, the hint "Me too." made the server drop those words
  ("I love pizza."); `turbo`, `large-v3-turbo` and `/asr` kept them. The hinted pass only runs after
  a miss, so the worst case is an answer that is not rescued.
- The panel's *Reconhecimento* row offers a server only once *Servidor* has an address (a server
  without one would not load). A typed address ending in `/v1` picks `openai`, others `asr`.
- Switching from a missing local model to a server in the panel takes effect for listening after
  a restart (`can_listen` is decided when the speech worker starts).

## Recognition on Groq's cloud (`stt_engine = "groq"`, #94)
- **Request.** The `openai` shape against `https://api.groq.com/openai/v1` with
  `model = whisper-large-v3-turbo` and `Authorization: Bearer {stt_key}`
  (https://console.groq.com/docs/speech-to-text, https://console.groq.com/docs/api-reference).
  Address and model are built in: `stt_url`/`stt_model` stay for the learner's own server, so
  switching between Groq and it needs no editing.
- **Key check before a lesson.** `Remote::check_key` is `GET {url}/models` with the key:
  200 = taken; 401/403 = `Refusal::KeyRefused`; no key = `Refusal::KeyMissing` without a request.
  The panel runs it (`Job::CheckKey` → `SpeechEvent::KeyWorks` / `Refused`) when a key is saved or
  Groq is picked with one; `doctor` runs it whenever a key is set.
- **Refusals by name.** 401/403 and 429 (`Refusal::Limited`) during a lesson arrive as
  `SpeechEvent::Refused` and are said with `text::refusal` in the learner's language; the turn goes
  to self-confirmation like any failed recognition.
- **The key is never shown whole.** `ApiKey`'s `Debug` and `mask_key` keep its first and last four
  characters (`gsk_…WXYZ`); the panel shows stars while it is typed, and a new key starts from an
  empty field. It is stored in plain text in the config file.
- **Panel.** *Groq (nuvem)* is always offered in *Reconhecimento*; picking it without a key opens
  *Chave*. Ctrl+V pastes into any open field (`arboard`, text only); on GNOME Wayland pasting may
  fail, which the panel says, and `stt_key` can be written in the config file instead. The panel is
  one row taller (224 px) for the *Chave* row.
- **Not measured.** No answer has gone through Groq from this repository: a key is the learner's.
  Free-plan limits for Whisper were not readable on Groq's rate-limits page (the Developer plan
  lists 20 requests per minute and 7,200 audio seconds per hour).

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
