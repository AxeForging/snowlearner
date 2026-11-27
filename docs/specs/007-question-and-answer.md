# 007 — Question and answer, with spaced reinforcement

Status: implemented (#220, requested by the learner on 2026-11-27). Builds on 002 (lessons,
tiers, the fight) and 005 (the path).

## Why
Every lesson asked the learner to *say* a phrase: heard first (repeat) or from the pt-BR
situation (recall). Real conversation goes the other way: someone asks you something in the
language and you answer. The learner also asked for reinforcement "like Anki": a missed
question, or one still to improve, comes back; won questions become small conquests, so
progress is visible. Before this, a phrase was "known" after 2 successes, forever, with no
spacing in time (`learn/path.rs`, `001` left spaced repetition out).

## What changed
1. **The exercise** — `exercise = "phrases" | "questions"` (config and panel, "Exercício").
   `phrases` is the game as before. `questions` asks only phrases that carry an `ask`, within
   the chosen topics and `max_level`, in `learn/path.rs` order.
2. **Deck fields** — `ask` is the question someone in the situation says, in the target
   language, never translated, unique within a deck. `family` (lowercase ASCII) ties one
   situation's rungs across levels: one topic, one phrase per level, no gap. In a cue, `{?}`
   plays the `ask`; a phrase with an `ask` and no cue gets `situation {?} Diga: {}`. The
   interview cues now play their question through `{?}`, not a second copy.
3. **What is spoken and shown** — only the language: the question, and in the first boxes the
   answer to repeat. Up to A1 the question is also written; from A2 it is only heard until the
   learner answers. After a miss the caption shows the situation (pt-BR), the question, the
   answer and its meaning, and the retry is a repeat, as recall already did.
4. **Boxes** (`learn/schedule.rs`, pure, rebuilt from the history on every pick):
   - boxes 0-5, `BOX_DAYS = [0, 0, 1, 3, 7, 21]`: box 0 never asked, box 1 back the same day,
     box 5 conquered and still back every 21 days;
   - **clean** (right on the first try, at the tier `answer` asks, `all` = complete; from box 2
     it must be recalled): up one box. **improve** (a later try, a shorter tier, or repeated
     where it had to be recalled): same box, same gap. **miss** (every try failed, or the
     learner left after a miss): down one box, never below 1, back first;
   - answering before it is due is extra practice: a clean answer then keeps the box;
   - order: missed, then due (lower box, then the longest unseen), then new ones, then extra
     practice on the one seen longest ago; never the same question twice in a row. New ones
     open while fewer than `LEARNING_SLOTS` (4) sit in boxes 1-2, at most `NEW_PER_DAY` (5) a
     day. A new question stays in boxes 1-2 until the next day, so in one day the 4 slots are
     the tighter limit.
   - `practice = auto`: boxes 0-1 repeat, box 2 and up recall; `repeat` always repeats (so a
     question stops at box 2); `recall` always recalls. A pre-A1 learner always hears the
     answer first only in boxes 0-1, so their questions can climb.
5. **What the learner sees** — after a question: "caixa 2 → 3 · volta em 3 dias"; reaching box
   5: "Conquistada! <question>"; the first question after starting or changing the exercise:
   "Hoje: N para revisar, M novas". None of it melts snow: melting stays in the answer's
   spell (`celebrate`), and a conquest counts once toward the sun and the comet like any
   right answer.
6. **History** — `attempts` gains `exercise`, `ask`, `tier`, `try_number`, `mode`, added with
   defaults when an older history opens. Question answers still count for their phrase
   (`stats` by `say`), so a phrase answered as a question also climbs the path.
7. **First content** (English for pt-BR speakers, reviewed by `language-teacher`, interview
   drafted by `job-interviewer`): four `restaurante` families (`restaurant-drink`,
   `restaurant-table`, `restaurant-order`, `restaurant-meal-end`) and four `entrevista`
   families (`interview-location`, `interview-intro`, `interview-role`,
   `interview-experience`), each PRE-A1 → B1. Every interview item has an `ask`. PRE-A1
   questions are choices that contain the answer ("Fries or salad?"), yes/no about the moment
   ("Can you hear me?") or a personal detail ("Where are you from?").

8. **Moving up a level** (#224, asked by the learner on 2026-11-27) — `schedule::level_up`
   measures the questions at `max_level` within the chosen topics: with 6 or more, at least
   80% in box 3 or higher; with fewer (pre-A1 has 4 per topic), all of them; and every topic
   with a question at that level has one there. Box 3 means answered clean on two separate
   days. The game **asks, never moves**:
   - after a right answer to a question, at most once a day (`level_up_offered_day` in the
     history's `meta`), a toast ("Você reforçou bem o pré-A1! Quer subir para A1? Veja no
     painel.") and the warrior saying what changes: leaving pre-A1 the voice gets a little
     faster (`target_speed`), at A2 the question is only heard, at B1 the answers get longer;
   - the panel's JOGO tab shows "Subir de nível?" in the status line's place while it holds;
     ← → pick "sim → A1" or "agora não", Enter (or a click on one) confirms. Yes is the level
     row's own change (saved and applied as usual); "agora não" keeps the offer away for
     7 days at that level (`level_up_declined` = "<day> <level>");
   - lower-level questions keep their boxes and stay in rotation (the level is a ceiling);
     nothing is offered at B1 or above, and the offer melts nothing.

9. **A family's next rung first** (#225) — among new questions, one whose family's rung one
   level below is reinforced (box 3, `REINFORCED_BOX`, the same mark moving up uses) opens
   before the others; otherwise path order. "Water." reinforced brings "Just water, please."
   next. It changes only which new question opens: misses and due reviews still come first,
   the slot and daily limits still hold, and `max_level` still caps the rungs.

## Not in this change
More topics, es and pt questions,
"conquered X/Y" in PROGRESSO and the streak (006 R13), personal answers from settings.

## Sources and judgment
The growing gaps follow Cepeda et al. 2006 (the best gap grows with how long you must
remember); conquered questions keep coming back because retrieval practice is what keeps
them (Karpicke & Roediger 2008). The intervals, `NEW_PER_DAY`, the grades and the level-up
thresholds (6, 80%, box 3, 7 days) are the game designer's judgment. Word and grammar levels were not checked against the English Vocabulary
or Grammar Profile (not reachable by automated clients); they rest on comparable deck items
and the CEFR Companion Volume (2018) descriptors the language teacher quoted.

## Tests
Schedule (box moves, intervals, extra practice, pick order, daily and slot limits, modes,
the level-up condition: share, every topic, fewer than six, current level only, B1 top; a
family's next rung first, still within the slot and daily limits, and in the English deck),
history (columns kept, migration of an old file), deck (`{?}`, unique asks, families, three
families per first topic, interview cues), lesson (only questions, only the language spoken,
box footer, miss reveal, pre-A1 recall from box 2, written up to A1, conquest melts no more,
no question in the filters; the level-up offer once a day, 7 days after "not now", melting
nothing, lower boxes kept), panel (the offer row in the status line's place, yes as a level
change, not now changing nothing, clicks), matcher ("I live in Canada" never passes for "I live in
Brazil."), binary (`exercise = "questions"` accepted; an old history still counts).
