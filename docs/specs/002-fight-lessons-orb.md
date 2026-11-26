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
   with ≥ 3/4 of the words. "I'm angry" never passes for "I'm hungry". A pre-A1 learner
   gets one more slip, only in words of 5+ letters (two slips turn "tea" into "see" and
   "wait" into "what"). In a short, polished or accepted answer, the words it shares with
   the phrase itself (`say`) must reach that 3/4 on their own — all of them when fewer than
   four — so "See for me please." never passes for "Tea." (#68). Those words get **no slip**
   at any level: each must be heard exactly (case, punctuation, accents and the deck
   language's sound folds aside). An answer form sharing no word with `say` ("Restroom?" for
   "Bathroom?") stands in for it and is held exact the same way. Whisper's slips are other
   real words, so a slip on the lesson's word taught the wrong one: in the learner's practice
   (2026-11-14 to 11-17) "bedroom" passed for "Bathroom?", "smile" for "Small." (twice),
   "heaven" for "Seven." and "eighty" for "Eight." — the last even without the beginner's
   slip. A right answer misheard this way gets the hinted second pass, which spelled the
   exact word in 13 of 16 misses (#80). The slips above still apply to the other words of a
   longer answer ("please" heard as "pleese"). When such a slip is refused, the caption
   names it under "Ouvi:": `Ouvi "eighty (80)" no lugar de "Eight"` — a number whisper
   wrote in digits is named as the word, so the learner sees another number, not a
   formatting slip. After any miss the phrase's tip shows too, and stays through the
   next tries until the phrase passes or the lesson moves on (#102).
5. **The fight** — the fire mage appears only during lessons, charges only while your voice
   is heard (an open mic in silence charges nothing, a pause holds it; shown only, #112),
   with a real flame on the staff that rises fast as you speak, holds and pulses while
   the answer is understood, goes out in smoke on a miss and becomes the fireball (#140),
   and a correct answer casts a fireball that melts only `melt_fraction` (12–25% by
   level). A miss fizzles and the frost mage fires back. 3 in a row call the sun
   (big melt, never everything; frost mage stunned; no snow falls under it, and the flakes
   still in the air melt into steam within 1.5 s, adding nothing to the pile, #171;
   on every other monitor too, with the same melt, the same steam and its own sun at
   the top, #188). 6 in a row (12, 18…) call a comet
   instead (#114): it falls on the frost mage (#135), melts 25% on impact and leaves
   4–6 little fire people (#139), thrown in arcs across the screen, one per slot of its width so
   the left, middle and right each get some (#172), with 24–30 burning chunks that bounce and
   fade; once landed they dash to the deepest, nearest snow arms flailing,
   run across it melting, then dash to the next patch (now and then a hop), from a
   budget of 37% of the snow left, for about 12 s; on bare ground they melt nothing. In all it never melts more
   than the sun; the frost mage is stunned as long. Daily goal + combo in the corner.
   New snow sticks less the deeper the pile (#170): at the ice line a cube, icicle or
   flake leaves only 35% of what it would on bare ground (`DEEP_PILE_SLOWDOWN`), so at
   Steady (one cube every 6 s) a clear screen took about 48 min to freeze at the edges
   and one right answer there kept it clear for about 11 min (it was 26 and 3.3).
   The pile grows only from snow you see land (#180): a cube leaves its own 36 px, an
   icicle 18, an owl's snowball 5, and a friend's spray leaves none on the ground (its
   frost on the edges stays). It snows like real snow, as hard as the commitment says
   and however buried the screen already is (`Pace::snowfall`, a share of a pool of one
   flake per 300 px): light at Chill, moderate at Steady, heavy at Committed and
   Relentless, in the ratio of real visibility classes (over 1 km, 0.5-1 km, under
   0.5 km: 1 : 2 : 4). The mage throws a cube every 30 / 15 / 6 / 3 s. Only the
   commitment's `Pace::settle_share` of whatever lands stays (#187: 0.5 / 0.5 / 0.8 /
   0.8; a flake stands for less snow, the snowfall looks the same), so on 480x270 a
   clear screen freezes at the edges in about 4 h at Chill, 2 h at Steady, 39 min at
   Committed and 29 min at Relentless, one right answer keeps it clear for about 75 /
   32 / 8 / 5.5 min, and the flakes lay 55-60% of the pile (44% at Relentless). Real snow piles up under 1 cm an hour (light) to
   10 cm in 12 h (heavy), about 0.1 px an hour at the scene's ~10 px per metre, so each
   flake stands for far more snow: the fill is time-compressed, the look is not. The
   wind follows the commitment (#183, `Pace::wind` ± `Pace::gust`, at ~10 px per metre):
   a light breeze at Chill (10 ± 5 px/s, ~1 m/s), a breeze at Steady (20 ± 12, ~2 m/s),
   a fresh wind at Committed (50 ± 25, ~5 m/s) and a blizzard at Relentless (160 ± 50,
   the NWS's 35 mph). It holds a side for about five minutes and turns over in a few
   seconds (`WIND_TURN_S` 600), with gusts over 3-7 s. The snowfield goes on past the
   screen: a flake blown off one side comes back in on the other at the same height,
   unless that wall has snow to slide down or hold it, so the flakes stay spread over
   the whole width and the fill times above hold.
   Where a fireball lands, a brief local dithered flash sized by the spell's tier
   (6/10/16 px, never the whole screen, melts nothing). When a phrase becomes known the
   fire mage flares (full flame and a ring of sparks) and the warrior cheers, once per
   phrase (#119).
6. **More frost mage** — asks the questions and taunts/groans in bubbles; summons
   friends early and often; icicle rain from the top of the screen. When you have been
   away (the idle nudge) and the pile is deep (`owl::PILE_LEVEL`), he rides a snowboard
   down it and taunts in two bubbles, "Que pista boa de esqui!" then "Estuda {língua},
   hein?" (#117). The warrior slides on skis and one ski run in three ends in a tumble.
   All of it is a show: no snow moves.
7. **Warrior vs mobs** — frost slimes/bats come from the edges; the warrior fights with his
   sword (bites chill him, wins warm him). A right answer finishes the mob in sword reach
   (#113); with none in reach and a bat the nearest mob, he draws a bow and an arrow
   flies an arc to the bat and brings it down (#118). No snow moves.
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
