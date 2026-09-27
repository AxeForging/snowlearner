# 002 — Real-life lessons, the fire mage fight, orb & black-hole pause

Status: implemented (requested live in chat, 2026-09-27). Builds on 001.

## Why
The MVP felt like a toy: phrases were Duolingo-style, one answer melted the whole
screen, and there was no way to tweak it while running (hotkeys didn't work on Wayland).

## What changed
1. **Lessons for real life** — decks carry `topic`, `level` (CEFR), a pt-BR `situation`,
   accepted variants (`accept`) and `tip`s; deck-level `[[tip]]`s (false friends,
   pronunciation traps for pt-BR speakers). 257 phrases per language ported from
   learn-lang-game (Lexicaster) by `scripts/port-lexicaster.mjs`, merged with the
   curated `decks/<lang>.toml` (work/remote meetings, travel, restaurant, health…).
2. **Practice modes** — new phrases: hear & repeat. Known phrases (2+ successes):
   recall from the pt-BR situation alone; a miss reveals the answer and retries as repeat.
3. **Retry tracking** — a phrase whose last attempt failed jumps the queue (never twice
   in a row) until you get it right; auto mode drops it back to repeat.
4. **Fair recognition** — each word may have 1 slip (3–6 letters) or 2 (7+), punctuation
   and accents ignored; pass when every word is heard, or when the phrase is very similar
   with ≥ 3/4 of the words. "I'm angry" never passes for "I'm hungry".
5. **The fight** — the fire mage appears only during lessons, charges while you speak,
   and a correct answer casts a fireball that melts only `melt_fraction` (12–25% by
   level). A miss fizzles and the frost mage fires back. 3 in a row call the sun
   (big melt, never everything; frost mage stunned). Daily goal + combo in the corner.
6. **More frost mage** — asks the questions and taunts/groans in bubbles; summons
   friends early and often; icicle rain from the top of the screen.
7. **Warrior vs mobs** — frost slimes/bats come from the edges; the warrior fights with his
   sword (bites chill him, wins warm him).
8. **Control** — magic orb (overlay: its own draggable window; window mode: in-scene):
   click = practice, Ctrl+click = panel, right-click = pause. Panel window edits settings
   live and saves them. Pause = black hole: the frame trembles, then is warped into the
   orb; resuming spits it back out; the world is frozen meanwhile.
9. **Wayland hotkeys** — `snowlearner shortcuts install` registers GNOME custom shortcuts
   calling `say|summary|menu` (idempotent; `shortcuts remove`).

## Performance budget
Overlay at 30 fps, release build: ~4% of one core, ~140 MB RSS before the recognizer
loads (+~150 MB once whisper is used). Nothing runs while paused except the orb.

## Tests
Unit: deck merge/selection/levels, picker retry + modes, matcher slips/angry-vs-hungry,
lesson (recall reveal, variants, combo/sun, goal once, filters, live deck switch),
scene (fireball melts on impact only, sun not total, icicles, friends early, mobs fought,
pause swallows & restores, orb clicks), vortex warp, menu navigation/validity, GNOME
list merge. Binary tests unchanged; speech e2e unchanged.
CI is manual (`workflow_dispatch`) to save runner minutes.
