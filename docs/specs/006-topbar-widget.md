# 006 — Top-bar widget: minimize the frost mage to your menu bar

Status: approved (2026-10-16; the user approved the top-bar specs with the overlay kept). Builds on 002 (orb, nudges), 004
(CPU presentation), 005 (path, progress, setup).

## Problem
- The frost-mage overlay is the game, and it stays the main UI. Some moments still need
  it out of the way for a while: a screen share, a demo, a full-screen video. Today the
  only ways are pause (black hole, which stops the game) or quitting, and when people
  quit, the habit dies with it.
- The status bar already holds small widgets that people glance at. A GNOME Shell 48 top
  bar with Freon shows `🌡36°C 🌡51°C`, for example. A `❄ 34% · 🔥 6d` next to them
  keeps the pressure where you look anyway, even while the mage is tucked away.
- Nothing shows a streak yet, even though `store/history.rs` has every attempt by `day`.

## Approach (summary)
The overlay and the lesson UI are never removed. The top bar is an **addition**: a place
to minimize the overlay to and show it from, which also shows the frost level, the
practice streak and a "lesson due" marker.

- Overlay visibility becomes a runtime state, `shown | minimized`, persisted across
  restarts. Left click on the top-bar item toggles it, as do `snowlearner toggle` and a
  "minimize to top bar" control in the app.
- Minimized means the windows are gone and nothing is drawn. **The game keeps going:** the
  frost keeps rising on the same game clock, the top bar shows it rising and goes hot, and
  at `wake_at` (60% by default) the mage comes back on his own. Minimizing buys time. It
  is not an escape.
- Every client reads one file the app writes, `<state_dir>/topbar.json`. The format is a
  contract shared with familiar and quasar.

## Shared contract (identical in familiar, quasar, snowlearner)

**1. The main UI stays.** The frost-mage overlay and lesson UI are never removed. The top
bar is a place to minimize to and show from. The top-level `mode = auto|window|overlay`
is untouched.

**2. Config.** `[topbar] enabled = false` by default. There is no `[topbar] mode` key.
- `snowlearner topbar install` installs the platform client **and** sets `enabled = true`
  through the comment-preserving config writer, and says so.
- `snowlearner topbar uninstall` removes the client **and** sets `enabled = false`, and
  says so.

**3. Visibility state.** `shown | minimized`, a runtime state persisted in the data dir
across restarts.
- Toggled by a top-bar left click, by `snowlearner toggle` (a new IPC command) and by a
  "minimize to top bar" control in the app. The app control exists only when
  `enabled = true`.
- Minimized means the windows are hidden and rendering is fully idle: zero frames.

**4. Client menu** (GNOME popup / SNI / NSStatusItem / Windows tray right-click):
- "Show snowlearner" / "Minimize": one item whose label follows the state
- "Start lesson"
- "Quit"

Left click = toggle.

**5. State file.** The app writes `<state_dir>/topbar.json` atomically: a temp file in the
same directory, then `rename`. Schema 1:

```json
{
  "schema": 1,
  "app": "snowlearner",
  "pid": 41237,
  "updated_at": 1792141200,
  "overlay": "minimized",
  "text": "❄ 34% · 🔥 6d · 📖",
  "level": "warn",
  "stale": false,
  "items": [
    {"key": "frost",  "label": "❄", "value": "34%", "level": "warn"},
    {"key": "streak", "label": "🔥", "value": "6d",  "level": "ok"},
    {"key": "due",    "label": "📖", "value": "",    "level": "warn"}
  ],
  "tooltip": "multi-line text\nalready in the learner's language",
  "click": ["snowlearner", "toggle"]
}
```

- `updated_at` is epoch seconds of the last write. It is both a change stamp and a
  heartbeat.
- `pid` is the writer's process id.
- `overlay` is `shown | minimized`. Clients use it for the toggle item's label.
- `level` is one of `ok | warn | hot | idle`.
- `click` is an argv. Clients run it without a shell.
- Readers ignore unknown fields. When the file doesn't parse, readers keep their previous
  display, so a half-written file is never shown.
- `text`, `level`, `tooltip`, `click`, `pid` and `overlay` are frozen for schema 1. A
  future schema may only add fields.

**6. Liveness.**
- The writer writes on every content change, and in any case **at least every 10 min**
  (heartbeat), even when nothing changed.
- Readers treat `updated_at` older than **30 min** as stale: greyed out, with the tooltip
  "not running?".
- On a clean exit the writer writes `stale: true` and `level: "idle"`.
- Clicking a stale item (flagged or aged) runs `["<app>"]`, which starts the app, instead
  of `click`.

**7. CLI.**
- `snowlearner topbar [--json]` prints the current state.
- `snowlearner topbar install|uninstall`: see point 2.
- `snowlearner toggle` flips the state.
- `snowlearner doctor` reports the widget.

**Clients.**

| Desktop | Client | Text in bar | Colour |
|---|---|---|---|
| GNOME Shell 45+ | in-repo extension `extras/gnome/snowlearner@axeforging.dev/` (GJS, ESM) watches the file with `Gio.FileMonitor`; installed to `~/.local/share/gnome-shell/extensions/` + `gnome-extensions enable` | yes | CSS class per `level` |
| KDE / other Linux | StatusNotifierItem published by the app (`ksni`, pure D-Bus, no GTK) | icon (`title` + tooltip) | icon tint |
| macOS | `NSStatusItem` with a text title, from the app | yes | attributed title |
| Windows | notification-area icon + tooltip, from the app | no (tooltip) | icon tint |

**Rejected for all three apps.**
- A top-bar-only mode without the overlay. The user wants the overlay kept as the main UI.
  The top bar is a place to minimize to, not a replacement.
- The `tray-icon` crate. It pulls GTK/libappindicator, and GNOME shows no text from it.
- One shared multi-app extension. It would tie three repos and two orgs to one release
  train.
- Polling in clients. Readers wake on file events, plus one one-shot staleness timer.
- "Write on change only, no heartbeat". A crashed app would leave a live-looking widget
  forever.

## Game design decision: minimizing must not dodge the game

**Decision.** Minimizing hides the mage. It does not stop him.
- While minimized, the frost keeps building at the commitment's pace, on the same game
  clock.
- The top bar shows the number climbing: `warn` from 30%, then `hot`.
- At `[topbar] wake_at` (default 60%), the overlay comes back by itself and the mage says
  so.
- You cannot minimize at or above `wake_at`, or during a lesson. The app shows a toast
  instead ("too frozen to hide — practice first").
- Pause (black hole) is unchanged. It is still the way to stop the clock, and while paused
  the frost doesn't grow, minimized or not.
- Only lessons melt the frost. "Start lesson" from the top bar shows the overlay for the
  lesson, and each correct answer melts `Pace::melt_fraction` as usual.

## Requirements

**R1. Config.**
- `Settings` gains `topbar: TopbarSettings { enabled: bool, wake_at: u8 }`, serialized as
  a `[topbar]` table. The serde defaults are `enabled = false` and `wake_at = 60`.
- An old config with no table loads with those defaults.
- `wake_at` outside 10–100 is rejected naming `topbar.wake_at`, the same way as the other
  settings.
- An unknown key in the table (including the old draft's `mode`) is rejected naming the
  key, as today.
- The top-level `mode` is untouched.

**R2. Disabled means today's app, exactly.**
- With `enabled = false`, no `topbar.json` is written.
- No new thread, D-Bus name, status item or tray icon.
- No minimize control in the app.
- Same windows as today.
- A persisted `minimized` state is ignored and reset to `shown`, so the overlay is never
  hidden without a way back.

**R3. Comment-preserving config writer.**
- `config/edit.rs::set(path, "topbar.enabled", true)` edits one key with `toml_edit`
  (already in `Cargo.lock` through `toml`; it becomes a direct dependency). It keeps every
  comment, the key order and the `HEADER`, and creates the table when it is missing.
- A missing file is created from `Settings::write_default` first.
- `Settings::save` (the panel's full rewrite) is unchanged.

**R4. Visibility state (pure + persisted).**
- `app/visibility.rs` holds `Visibility { Shown, Minimized }` and the pure transition
  `toggle(state, Ctx { enabled, frost, wake_at, in_lesson }) -> Result<Visibility,
  Refusal>`.
- Refusals: disabled; `frost ≥ wake_at` when minimizing; in a lesson. Showing is never
  refused.
- Persisted as `<data_dir>/overlay.json`, `{"overlay": "minimized"}`, written atomically
  on every transition and read at startup. A missing or unreadable file means `shown`.

**R5. Minimized = zero frames.**
- Minimizing destroys the main, orb and weather `Surface`s, which frees their canvases. It
  does not just hide them. The open control panel stays.
- `about_to_wait` requests no redraws while minimized. The loop sleeps with
  `ControlFlow::WaitUntil(next_wake)`, where `next_wake` is the earliest of:
  - the next whole-percent frost crossing (from the curve, R6)
  - the `wake_at` crossing
  - the heartbeat (R10)
  - the lesson nudge (`ask_every − idle_for`)
  - the daily summary time
- IPC, hotkeys and SNI/tray events still wake the loop through the `EventLoopProxy`.
- Showing recreates the surfaces through the same path as `resumed`/`relayout`, for both
  the window and overlay top-level modes.

**R6. Frost across a minimized span.**
- **Why analytic is not possible.** Freeze growth is not a function of time. It emerges
  from the seeded simulation: cubes thrown on rng timers, mobs, icicles, friends' frost
  bursts, the warrior's campfires hollowing the pile, then edge creep past the ice line
  (`Scene::step`, `freeze_level = 0.6·pile + 0.8·edges`). Simulating it undrawn would cost
  CPU for the whole span, and replaying it in a burst on restore would cost tens of
  seconds of CPU for an hour minimized.
- **Cheapest correct alternative.** A calibrated curve plus a one-shot catch-up.
  - `scene/catchup.rs` holds a per-`Commitment` table of the **mean** `freeze_level`
    against game seconds. The table is measured from the real `Scene` (8 seeds, `dt = 0.1`,
    reference size `PACE_HEIGHT`, no lesson) and checked in.
  - `advance(level, secs, commitment) -> f32` finds `level` on the curve, moves `secs`
    along it, and interpolates linearly. It is monotone, never decreases, and clamps to 1.
  - `secs_until(level, target, commitment)` gives the wake deadlines in R5.
- **Same game clock.** Elapsed time counts only while the scene is not paused (the black
  hole stops the clock, as in `Scene::step`). The lesson gets the real elapsed time on each
  wake: one `Tick` with an uncapped `dt`, because the 0.1 cap is a physics cap for the
  scene only. That way `due`, the streak day and the summary keep running.
- **On show.** `Scene::catch_up(target)` sets the pile and edge snow straight to `target`.
  It reuses the pile/edges split `step_weather` already uses (pile first up to the ice
  line, then the edges), extracted as `drift_to(target, k)` and called with `k = 1` until
  within 0.5 points (bounded loop). Actors, timers and the rng are left as they were.

**R7. Auto-restore at `wake_at`.**
- When the estimated frost reaches `wake_at` while minimized, the state becomes `shown`
  (persisted), the scene catches up to exactly that level, and the mage says
  `T::MageBack` ("Did you think I'd melt? Back to practice.").
- This happens at the computed deadline, not at the next percent tick.

**R8. Toggle IPC + CLI.**
- `Command::Toggle` (`"toggle"`) is added to `control/ipc.rs`. The app applies R4. A
  refusal is shown as a HUD toast on the overlay, since the IPC ack is sent before the app
  handles the command.
- `snowlearner toggle`:
  - reads `[topbar].enabled` first; when it is false, it exits 1 with "enable the top bar
    first: `snowlearner topbar install`"
  - with no running app, it exits non-zero with the existing "not running" message
- The in-process clients (SNI, macOS, Windows) skip the subprocess and send
  `UserEvent::Ipc(Command::Toggle)` through the existing `EventLoopProxy`.

**R9. In-app control (only when enabled).**
- The panel's GAME tab gets a `Minimize to top bar` action item.
- The `b` key on the main window does the same.
- Both go through R4, so refusals give the same toast.

**R10. Writer.**
- When enabled, the app evaluates the widget at most once per second while shown. It runs
  on the existing frame clock: no new timer and no new thread.
- While minimized, it evaluates only at the R5 wakes.
- It compares the serialized state with the last written bytes, leaving out `updated_at`.
  It writes when they differ, **or** when the last write is ≥ 10 min old (heartbeat).
- It writes atomically: `topbar.json.tmp` in the same directory, then `rename`, which
  replaces the file on Windows too.
- A fresh state is written on startup.
- On `Quit` and on a clean exit it writes `stale: true` and `level: "idle"`.
- `<state_dir>` is `Paths::state_dir()`, which is `data_dir`:
  - `~/.local/share/snowlearner` on Linux
  - `<SNOWLEARNER_HOME>/data` in tests
- A write error is logged once and the app keeps running.

**R11. State model (pure).** `topbar::widget::build(&Inputs) -> Widget` turns plain inputs
into the schema-1 struct.
- Inputs: freeze 0..1, edges frozen, visibility, streak, due, paused, `Progress`, daily
  done/goal, freeze ETA, `wake_at`, pid, native language.
- `text`:
  - Normally `❄ {pct}% · 🔥 {n}d`, plus ` · 📖` when a lesson is due.
  - When paused: `❄ {pct}% · ⏸`.
  - When stale: `❄ — · 🔥 {n}d`.
- `pct` is the freeze level rounded **down** to a whole percent, clamped to 0–100.
- `click` is always `["snowlearner","toggle"]`.

**R12. Level rule.**

| Level | When |
|---|---|
| `idle` | paused or stale |
| `hot` | freeze ≥ `wake_at − 15` (45% by default), or the edges are frozen over (`Scene::edges_freeze`) |
| `warn` | freeze ≥ 30%, or a lesson is due |
| `ok` | otherwise |

The bar is hot for a while before the mage returns. Each item also carries its own level:
- streak: `ok`, or `warn` when today has no success yet and the streak is still alive
- due: `warn`

**R13. Streak.**
- A streak is the number of consecutive local days, for the current `learning` language,
  with at least one successful attempt. It ends today, or yesterday if today has no
  success yet, so the streak is alive until midnight.
- 0 when neither today nor yesterday counts.
- Computed by the pure `learn/streak.rs` from the days returned by
  `History::success_days(language)`.

**R14. Lesson due.**
- `Lesson` gets a `due` flag. It is set when the idle nudge fires (`idle_for >= ask_every`,
  the same moment as `T::ToastIdle`). That includes while minimized, through the uncapped
  wake ticks.
- The flag is cleared when a challenge starts. It is never set while paused.

**R15. Tooltip.** Lines in the learner's language. Every string lives in `lang/text.rs` and
is checked by `font::supports` like the others:
1. `Snowlearner — {language_name}`
2. path: the current stage and its known/total (percent), e.g.
   `Chunks: 18/30 known (60%)`; or "everything known"
3. next item: the first `Progress::learning` item, or else the first of
   `Progress::next_up`, as `say — meaning`
4. today: `{done}/{goal}`
5. freeze:
   - shown: `next step 40% in ~6 min` when rising (R16), `melting` / `steady` otherwise
   - minimized: `minimized — mage back at 60% in ~12 min` (from `secs_until`)
   - `paused`
6. the click hint: `click to show` / `click to minimize`

When stale, readers replace the tooltip with "not running?".

**R16. Freeze ETA while shown (pure).**
- `topbar::trend::Trend` keeps the (seconds, level) samples of the last 120 s, fed at most
  once per second.
- The next step is the next multiple of 10% above the current level. The ETA is the time
  to reach it at the least-squares slope, rounded **up** to whole minutes.
- `None` when there are fewer than 30 s of samples, when the slope ≤ 0, or when paused or
  in a lesson.
- While minimized, the curve (R6) gives the ETA instead.

**R17. Start lesson from the top bar.**
- `snowlearner lesson` is a visible alias of `say` (IPC `Command::Challenge`). The GNOME
  menu runs it. In-process clients send `Command::Challenge` directly.
- When minimized, a challenge first shows the overlay (catch-up, R6) and starts the
  lesson. When the lesson goes back to `Idle`, the overlay returns to `minimized` if it
  was minimized before and the frost is now below `wake_at`. Otherwise it stays shown.
- Melting is the usual `melt_fraction` per correct answer.

**R18. Client choice (pure).** `topbar::client::pick(os, desktop)`:

| OS / desktop | Client |
|---|---|
| Linux + `XDG_CURRENT_DESKTOP` contains `GNOME` | `File` only (the extension reads it; no SNI, so nothing shows twice) |
| other Linux | `File + Sni` |
| macOS | `File + StatusItem` |
| Windows | `File + Tray` |

**R19. GNOME extension.** It lives in `extras/gnome/snowlearner@axeforging.dev/`:
`metadata.json` (shell-version 45–49), `extension.js`, `state.js` (the pure parser +
staleness rule) and `stylesheet.css`.
- **Display.** A `PanelMenu.Button` with an `St.Label` showing `text`, and the CSS class
  `snowlearner-{level}`. Stale states use `snowlearner-stale` (greyed).
- **Clicks.**
  - A left click runs the `click` argv with `Gio.Subprocess`, never a shell, or
    `["snowlearner"]` when stale.
  - A right click opens the menu with three items:
    - "Show snowlearner" / "Minimize", picked from `overlay`, which runs `click`
    - "Start lesson", which runs `["snowlearner","lesson"]`
    - "Quit", which runs `["snowlearner","quit"]`

  Labels come in pt-BR and en, picked from `GLib.get_language_names()`.
- **Watching.** A `Gio.FileMonitor` (`WATCH_MOVES`) on
  `$XDG_DATA_HOME/snowlearner/topbar.json`. It re-reads the file on `CHANGES_DONE_HINT`,
  `CREATED`, `MOVED_IN` and `RENAMED`. On `DELETED` the widget is hidden.
- **Staleness.** After each read, a one-shot `GLib.timeout_add_seconds` is scheduled at
  `updated_at + 30 min`, replacing the previous one. When it fires, the item is greyed.
  There is no repeating timer.
- **Parsing.**
  - A missing file hides the widget.
  - Unparseable JSON keeps the last display.
  - `schema` > 1 still uses the frozen fields.
  - `app` ≠ `snowlearner` is ignored.
- **Lifecycle.** `disable()` drops the monitor, the timeout and the button. Nothing leaks
  across lock/unlock.

**R20. `snowlearner topbar [--json]`.**
- **App running.** When the IPC greeting answers on `ipc_port` and the file is fresh, it
  prints the file. It prints `text` and `tooltip` without `--json`, and the raw JSON with
  it.
- **Stale.** When the file is older than 30 min, or `pid` is not alive, it reports stale.
- **App not running.**
  - It builds the state offline from history and decks.
  - Frost is `—`, `level: "idle"` and `stale: true`.
  - It never writes the file.
- Exit code 0 in all three cases.

**R21. `snowlearner topbar install`. Idempotent.**
- **GNOME.**
  - Writes the extension files, embedded with `include_str!`, to
    `$XDG_DATA_HOME/gnome-shell/extensions/<uuid>/`.
  - Runs `gnome-extensions enable <uuid>`.
  - If that fails (Wayland doesn't see a new extension until the next login), it adds the
    uuid to `org.gnome.shell enabled-extensions` through `gsettings`, without duplicates
    and keeping the others. It then prints "log out and back in".
- **Elsewhere.** It writes no extension files and says which client will show (KDE tray /
  menu bar / notification area).
- **Always.**
  - Sets `[topbar] enabled = true` through R3.
  - Prints `top bar enabled in <config path> — restart snowlearner to apply`, or "already
    enabled".
- **`SNOWLEARNER_HOME`.** When it is set, it warns that the extension only watches the
  default data dir.

**R22. `snowlearner topbar uninstall`. Idempotent.**
- Disables and removes the extension directory (GNOME).
- Removes the uuid from `enabled-extensions`, keeping the others.
- Sets `enabled = false` through R3 and says so.
- Resets `overlay.json` to `shown`.

**R23. Setup and installer.**
- `snowlearner setup --topbar` runs `topbar install` after the usual steps.
- On GNOME without `--topbar`, `setup` prints one line suggesting
  `snowlearner topbar install`.
- `install.sh` passes `--topbar` through (it already forwards `"$@"`). When `/dev/tty`
  exists and GNOME Shell is detected, it asks once ("Mostrar no painel superior? [s/N]"),
  defaulting to no. Non-interactive runs never change anything.

**R24. Doctor.** One `topbar` line:
- enabled / disabled, `wake_at`, and the current visibility
- the client `pick` chose
- for GNOME: whether the extension is installed and enabled (`gnome-extensions info`)
- the state file's age

It flags `!!` when:
- enabled on GNOME with no extension
- the file is stale while the IPC greeting answers

**R25. SNI (other Linux, `tray` feature, default on).**
- A `ksni` blocking service with:
  - `title` = `text`
  - `tool_tip` = `tooltip`
  - an icon from the frost-mage window icon (`app/mod.rs::icon`), tinted per level
    (4 pixmaps rendered once)
- `activate` maps to Toggle. The menu has the three contract items (Show/Minimize, Start
  lesson, Quit), which map to `Command`s.
- Updated only when the widget changes.
- Its D-Bus thread blocks on the bus and wakes only for calls and updates.
- Without a StatusNotifierWatcher on the bus, the app logs it once and carries on (file
  only).

**R26. macOS.**
- An `NSStatusItem` of variable length, created on the main thread inside winit's
  `resumed`.
- The button title is `text`, coloured per level with an attributed string.
- `toolTip` is `tooltip`.
- A left click maps to Toggle. The menu (right click / ctrl-click) has the three contract
  items.

**R27. Windows.**
- `Shell_NotifyIconW` on a message-only window created on the winit thread, so winit's
  loop pumps its messages.
- The icon is tinted per level. The tooltip is `text` plus the freeze line, truncated to
  the 127-char `szTip` limit.
- A left click maps to Toggle. The right-click menu has the three contract items.

**R28. Footprint.**
- **Minimized.**
  - Zero frames presented.
  - No scene stepping.
  - Wakes only at the R5 deadlines: about one per percent of frost, plus heartbeats.
  - Idle CPU is near zero, and RSS is lower than shown because the canvases are freed.
- **Shown + enabled.** At most one JSON write per change, about one per whole percent or
  per ETA minute, plus a heartbeat every 10 min.
- No polling anywhere.

## Non-goals
- Removing or replacing the overlay, or any top-bar-only mode.
- Desktop notifications or sounds from the widget.
- Showing the lesson itself (phrase, mic meter) in the bar. The overlay does that.
- Persisting the freeze level across restarts. The scene starts clear, as today. Only
  visibility is persisted.
- Pause and panel actions in the top-bar menus. They stay in the app; the menu is the
  three contract items.
- Extensions for GNOME < 45 (legacy imports), Cinnamon applets, waybar modules. Waybar
  users can `cat` the file; that is documented, not supported.
- Changing the top-level `mode` (`auto | window | overlay`) or any balance numbers.

## Design

### Modules
New domain dir `src/topbar/` (`mod.rs` = module list):

| File | Holds | Pure? |
|---|---|---|
| `widget.rs` | `Inputs`, `Widget` (schema-1 serde struct), `Level`, `build`, the level rule | yes |
| `trend.rs` | `Trend` (samples, slope, `eta_to_next_step`) | yes |
| `client.rs` | `Client` set + `pick(os, desktop)` | yes |
| `stale.rs` | `is_stale(updated_at, now)` (30 min), `HEARTBEAT` (10 min) | yes |
| `file.rs` | `Writer` (last bytes, last write, heartbeat, atomic write, stale-on-exit) | fs only |
| `extension.rs` | embedded extension files; install/uninstall/info via `gnome-extensions` + `gsettings` (like `control/gnome.rs`) | proc |
| `sni.rs` | `ksni` tray (`cfg(linux)`, feature `tray`) | D-Bus |
| `status_item.rs` | `NSStatusItem` (`cfg(macos)`) | AppKit |
| `tray.rs` | notification-area icon (`cfg(windows)`) | Win32 |

Other files:
- `scene/catchup.rs`: calibrated curves, `advance`, `secs_until`. Pure. In `scene/mod.rs`:
  - `Scene::catch_up(target)` and `drift_to`, extracted from `step_weather`
  - `edges_freeze` becomes `pub(crate)`
- `app/visibility.rs`: `Visibility`, `toggle`, `Refusal`, load/save of `overlay.json`, and
  `next_wake(...)`, which takes the deadlines as arguments. Pure apart from the file I/O.
- `app/mod.rs`:
  - `TopbarRuntime { writer, trend, clients, last_eval }`
  - `Minimized { since: Instant, base_level, restore_after_lesson }`
  - dropping and recreating the surfaces
  - the minimized `WaitUntil` path in `about_to_wait`
- `app/lesson.rs`: the `due` flag.
- `app/menu.rs`: `Item::MinimizeToTopbar`, listed only when enabled.
- `control/ipc.rs`: `Command::Toggle`.
- `config/settings.rs`: `TopbarSettings`.
- `config/edit.rs`: the `toml_edit` single-key writer.
- `config/paths.rs`: `state_dir()`, `topbar_file()`, `overlay_file()`.
- `learn/streak.rs`: `streak(days, today) -> Streak { days, alive_today }`. Pure.
- `store/history.rs`: `success_days(language)`, which runs `SELECT DISTINCT day … WHERE
  success = 1` and uses the existing `attempts_day` index.
- `cli/mod.rs` + `commands.rs`: `Topbar { json, action }`, `Toggle`, the `lesson` alias,
  `setup --topbar`, the doctor line.
- `lang/text.rs`: tooltip lines, `MageBack`, the refusal toasts, panel label, CLI
  messages, in both languages.

`scene/`, `learn/`, `app/visibility.rs` and `topbar/{widget,trend,client,stale}.rs` stay
free of windows and clocks: time comes in as arguments.

### Minimize / show flow
- **Minimize** (`toggle` → `Minimized`):
  1. Record `since` and `base_level = scene.freeze_level()`.
  2. Drop the main, orb and weather surfaces.
  3. Persist `overlay.json`.
  4. Write the widget (`overlay: "minimized"`).
- **Each wake while minimized:**
  1. `est = advance(base_level, active_secs, commitment)`.
  2. Feed the lesson one uncapped `Tick`.
  3. Run the writer.
  4. If `est ≥ wake_at`, auto-restore.
  5. Otherwise set `WaitUntil(next_wake)`.
- **Show** (toggle, auto-restore, or a challenge):
  1. `scene.catch_up(est)`.
  2. Recreate the surfaces (top-level mode as resolved at startup).
  3. Persist.
  4. Reset `last` so the first frame's `dt` is small.
- **Startup with `minimized` persisted and enabled:** no surfaces are created. The scene
  starts clear, as today, and the clock starts at `base_level = 0`.

### Rejected alternatives
- **A top-bar-only mode without the overlay** (the earlier draft's
  `[topbar] mode = overlay|topbar|both`, with a headless scene at 10 Hz and a temporary
  lesson window). Rejected: the user wants the overlay kept as the main UI, with the top
  bar only as a place to minimize to and show from.
- **Simulating the scene undrawn while minimized** (10 Hz, `dt = 0.1`). It burns CPU for
  the whole span when the goal is zero frames.
- **Replaying the simulation in one burst on show.** It would cost tens of seconds of CPU
  per hour minimized, and the wake deadline couldn't be known without it.
- **A closed-form freeze formula.** None exists: growth is rng-driven and emergent. The
  calibrated mean curve is the closest correct model, and a test keeps it honest (R6).
- **`set_visible(false)` instead of dropping the surfaces.** It is a no-op on native
  Wayland (window mode), and it keeps the full-screen canvases in RAM.
- **Minimizing while frozen past `wake_at`.** It would bounce straight back, or be an
  escape.
- **Reading the visibility back from `topbar.json`.** That file is an output and is
  absent when the top bar is disabled. The state has its own file, `overlay.json`.
- **`Settings::save` for install/uninstall.** It rewrites the file and drops the user's
  comments. Hence the `toml_edit` writer.
- **The `tray-icon` crate, a shared multi-app extension, polling.** See the contract.

## Data / API changes
- **Config.** A new `[topbar]` table with `enabled` (default `false`) and `wake_at`
  (default `60`, range 10–100). There is no `mode` key, and the top-level `mode` is
  unchanged.
- **Files.**
  - `<data_dir>/topbar.json` + the transient `topbar.json.tmp`, written only when enabled
  - `<data_dir>/overlay.json` (`{"overlay":"shown"|"minimized"}`)
- **`topbar.json` schema 1.**
  - Adds `pid` and `overlay`.
  - `click` = `["snowlearner","toggle"]`.
  - `updated_at` is now also the heartbeat: at least every 10 min, stale after 30 min.
- **History.** A read-only `success_days` query. No schema migration.
- **IPC.** `toggle` (`Command::Toggle`). The others are unchanged.
- **CLI.**
  - `toggle`
  - `topbar [--json]`
  - `topbar install`, `topbar uninstall` (these flip `enabled`)
  - `lesson` (alias of `say`)
  - `setup --topbar`
  - doctor `topbar` line
- **Cargo.**
  - `toml_edit` as a direct dependency (already in the lockfile)
  - `ksni` behind a new `tray` feature (default on, Linux-only target dep)
  - `objc2`, `objc2-foundation`, `objc2-app-kit` (macOS target deps)
  - `windows-sys` gains `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`
- **Repo.** `extras/gnome/snowlearner@axeforging.dev/`. The CLAUDE.md layout gets
  `topbar/`, `scene/catchup.rs`, `app/visibility.rs`, `config/edit.rs`,
  `learn/streak.rs` and `extras/`.

## Test plan
Behaviour-named tests, no mocks inside the crate.
- External tools (`gnome-extensions`, `gsettings`) are faked with recording scripts on
  `PATH` in binary tests: that is the external boundary.
- Time is injected as arguments (pure modules) or through the `SNOWLEARNER_CLOCK_OFFSET`
  test hook. The hook is compiled only with `cfg(any(test, feature = "test-hooks"))`.

| Req | Tests |
|---|---|
| R1 | Settings: no table → `enabled = false`, `wake_at = 60`; round-trip through save/load; `wake_at = 5` and `= 101` rejected naming `topbar.wake_at`; `[topbar] mode = "both"` rejected naming the key; top-level `mode` unaffected. |
| R2 | Binary (`tests/cli.rs`): default config → `snowlearner topbar` never creates `topbar.json`. Unit: `toggle` with `enabled = false` → `Refusal::Disabled`; a persisted `minimized` + disabled loads as `shown`. Menu: `MinimizeToTopbar` absent when disabled. |
| R3 | `config/edit`: a commented config (header, comments above and beside keys) → after `set`, every comment line and the key order are byte-identical except the changed key; the table is created when missing; a missing file is created from defaults; setting the same value twice leaves the file unchanged. |
| R4 | `toggle`: shown → minimized; minimized → shown; refused at `frost == wake_at` (boundary) but allowed at `wake_at − 0.01`; refused in a lesson; show never refused. `overlay.json` corrupt → `shown`. |
| R4 (restart) | Binary, **minimized persisted across restart**: seed `overlay.json = minimized` + `enabled = true`, start the app (fake display as in existing app tests, or a `--headless-check` exit-after-startup path), and assert the written `topbar.json` says `"overlay": "minimized"`. Then `toggle` + quit + restart → `"shown"`. |
| R5 / R28 | **Zero frames while minimized.** Unit: `next_wake` is the earliest of its deadlines, and while minimized the redraw plan requests no surface. Release measurement on Fedora 42 GNOME 48: the app logs `frames=N` on each visibility change; across a 10-min minimized span N is unchanged, the `/proc/<pid>/stat` utime+stime delta is < 0.2 s, and RSS drops versus shown. Recorded in the PR, as in 004. |
| R6 | **Frost across a minimized span (injected clock).** `advance` is monotone, `advance(l, 0) == l`, clamps at 1, and `advance(advance(l, a), b) == advance(l, a + b)` within 1e-4. Paused seconds don't count. Calibration guard: real `Scene`s (2 seeds per commitment, `dt = 0.1`, 15 sim-min, release-only `#[cfg_attr(debug_assertions, ignore)]`) stay within ±5 points of the curve. The table generator is an `#[ignore]` test that prints the table. `Scene::catch_up(0.45)` → `freeze_level` within 0.5 points, from 0 and from 0.7 (melting down). |
| R7 | **Auto-restore exactly at `wake_at`.** Pure: `secs_until(base, wake_at)` against `advance`; the wake deadline lands on the crossing second, and one second earlier stays minimized. App-level unit on the minimized controller with an injected `now`: at `deadline − 1 s` still minimized; at `deadline` shown, persisted, the scene caught up to `wake_at ± 0.5`, and `MageBack` toasted. |
| R8 | **Toggle round trip through the real binary.** Start a fake IPC listener as in `say_and_summary_reach_a_running_instance`; `snowlearner toggle` (enabled config) delivers `toggle`; disabled config → exit 1 with the install hint, and nothing is sent; no listener → "not running". `Command::parse("toggle")` ↔ `as_str` round-trip. |
| R9 | Menu: the item is present only when enabled and routes to `toggle`; `b` does the same; a refusal shows the toast text. |
| R10 | **Heartbeat 10 min.** Writer in a temp dir with an injected clock: same widget twice → one write; changed text → a second write; `updated_at` changes alone → no write; same widget at +9:59 → no write, at +10:00 → a write; no `.tmp` left; stale-on-exit content; unwritable dir → error once, no panic. |
| R11 / R12 | `build`: text for normal / due / paused / stale; 33.99% → `33%`, 1.0 → `100%`, -0.1 → `0%`; `pid`, `overlay` and `click == ["snowlearner","toggle"]`. Level table: 29/30%, `wake_at − 15` ± 1 with `wake_at` 60 and 80, edges frozen at 10% → hot, paused at 90% → idle, due at 0% → warn. |
| R13 | Streak: empty → 0; today only → 1; yesterday + the day before, none today → 2 and alive-not-done; one-day gap → resets; other language ignored (History on a temp DB); duplicate days counted once. |
| R14 | Lesson: the nudge sets `due`, including via one uncapped 600 s tick; a challenge clears it; paused longer than `ask_every` never sets it. |
| R15 / R16 | Tooltip in pt-BR and en for shown, minimized (with the ETA to `wake_at`), paused, empty history, everything known; every string passes `font::supports`. Trend: < 30 s → None; 1%/min at 34% → 6 min; falling → None; exactly on a step → the next one. |
| R17 | Lesson from minimized: the challenge shows the overlay; on `Idle` with frost below `wake_at` it is back to minimized; at or above `wake_at` it stays shown. |
| R18 | `pick`: GNOME, `ubuntu:GNOME`, KDE, empty desktop, macOS, Windows. |
| R19 | `gjs -m` test of `state.js` in CI (apt `gjs`): valid → text/level/tooltip/click/toggle label; **stale by age** (`updated_at` = now − 30 min + 1 s → live; − 30 min → stale, click becomes `["snowlearner"]`, tooltip "not running?"); `stale: true` → same; schema 2 with extra fields → still shown; bad JSON → previous kept; other `app` → ignored; missing → hidden. Rust: the embedded `metadata.json` uuid matches the dir name and shell-version covers 45–49. Manual on GNOME 48 (Wayland): left click toggles, right-click menu has the 3 items, `kill -9` the app, and after 30 min the item greys out, `disable()` is clean. |
| R20 | Binary: no app → offline JSON with `stale: true` and `level: "idle"`, exit 0, file not created; file with `updated_at` 31 min old + a dead `pid` → reported stale. |
| R21 / R22 | **Install sets enabled and uninstall restores.** Binary with `XDG_DATA_HOME`, `XDG_CURRENT_DESKTOP=GNOME`, fake `gnome-extensions` (exit 1) + fake `gsettings`, and a commented config: install → extension files written, uuid added once (twice is idempotent), `[topbar] enabled = true`, the comments intact, and the message names the config path. Uninstall → dir and uuid removed (others kept), `enabled = false`, comments intact, `overlay.json` = shown; run twice → ok. Non-GNOME → no extension files, still enabled. `SNOWLEARNER_HOME` warning. |
| R23 | Binary: `setup --no-model --topbar` installs (fakes as above); on GNOME without the flag it prints the hint. `tests/install.rs`: `--topbar` reaches setup; no tty → no prompt; shellcheck clean. |
| R24 | Binary: doctor prints a `topbar` line enabled and disabled; `!!` for enabled on GNOME without the extension (fake `gnome-extensions info` exit 1). |
| R25–R27 | Unit: widget → SNI props (title, tooltip, tinted pixmap per level, menu label from `overlay`); tooltip truncation to 127 UTF-16 units, never splitting a char. Manual on KDE Plasma (VM), Windows 11 (owned machine), macOS via a release build; no extra macOS/Windows CI runs (billed minutes). |

Removed from the earlier draft: the 10 Hz versus 30 Hz headless-scene test, the
"temporary lesson window" manual check, and the topbar-only CPU comparison.

## Rollout
1. **Pure core, invisible to existing users.**
   - `widget`, `trend`, `client`, `stale`, `streak`, `success_days`
   - the `due` flag, the `[topbar]` table, `config/edit.rs`
   - `scene/catchup.rs` with its calibrated tables and guard test
   - `app/visibility.rs`
   - the `lesson` alias and the offline `snowlearner topbar [--json]`
2. **Minimize/show in the app.** `Command::Toggle` + `snowlearner toggle`, the panel item
   and the `b` key, dropping and recreating the surfaces, the minimized `WaitUntil` path,
   catch-up, and auto-restore at `wake_at`. Measure the R28 footprint on the release build.
3. **Writer with heartbeat + GNOME extension (left click, menu, staleness timer) +
   `topbar install|uninstall` + doctor.** Try it on the GNOME 48 desktop it was asked for,
   including `kill -9` staleness.
4. **SNI (`tray` feature), then the macOS status item and the Windows tray.** Each one is
   checked by hand on its OS before the release.
5. **Release.**
   - `setup --topbar`, the `install.sh` prompt, README (Use table, Configure, Platforms
     row), CLAUDE.md layout.
   - Tag `v0.2.0`. `enabled = false` stays the default, so upgrading changes nothing until
     someone runs `topbar install`.

Rollback: run `snowlearner topbar uninstall`, or set `[topbar] enabled = false`. The overlay
is shown again, because a disabled top bar ignores a persisted `minimized`. The state file
is only read, never required.
