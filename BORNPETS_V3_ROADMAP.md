# BornPets v3 roadmap

Spec: `BORNPETS_V3_DESIGN.md` (repo root). Current rules and results: sections 15 to 19 (18 and 19 win where they differ from earlier sections; 17 and 18.8 are the owner's decisions).
Code base: worktree `../BornHack-Cyberegg-fable6`, branch `ranzbak/fable-06-layering`, HEAD `710371f`. All paths and line numbers in the plans refer to that tree. v3 lands after that cleanup.
Each plan ships working firmware and a working simulator on its own, with its tests green.

## Order

| # | Plan | Ships | Depends on | Spec sections | Size |
|---|---|---|---|---|---|
| 1 | **Engine core and save v3** | the v3 needs, doses, interactions, conditions, sleep, delta solver, 2 h wake cap, save v3 with the save format 2 upgrade, simulator with step oracle; just enough menu items to play it (Wash, Relax, Wake up, Water refusal toasts) | none | 1 to 4, 13, 15.1 to 15.6, 15.8, 17 rows 1 to 10 and 28 to 31, 18.1 to 18.3 | L |
| 2 | **Reading the pet (UI)** | two-page stats view with band ticks, status line, cause toasts, menu hints, diary, Health status entry removed, 5-minute idle frame | 1 | 5, 17 rows 29 and 32, 18.6 | M |
| 3 | **Demo mode** | self-caring pet, clock-following sleep, fitness floor, weight control (owner: tune here; one run ended at 98 %), settings toggle, calm display; updates guide ch 3 and 5 | 1 (engine events), 2 (status line) | 8, 13.3, 17 rows 11 to 15, 18.3, 18.8 | M |
| 4 | **NFC stations** | `fix s=...` plaintext, old phrases as aliases, per-station cooldown, clinic preset; updates guide ch 11 | 1 | 19 | S |
| 5 | **Battles** | new battle formula and floors, snapshot, challenge / accept / decline, pre-battle screen with win chance, 100 / 20 HAX pay, beacon with six numbers; updates guide ch 11 | 1; 2 for the mirrored bar widget | 12, 15.7, 17 rows 16 to 23a, 18.4, 18.5, 18.8 | L |
| 6 | **Hard mode and money** | v3 prices and work, debt, deductible and copay, work insurance, insulin creep (cap 60), surprise bills, authorization popup, eviction; updates guide ch 9 and 10 | 1, 2 (bill and popup screens) | 15.9, 16, 17 rows 24 to 27, 18.5 | M |
| 6b | ~~Crowdfunding and cross-border pharmacy~~ **DROPPED** (owner, 2026-11-14) | not built | n/a | 16.3 rows 10 and 11 | n/a |
| 7 | **Backgrounds, floor items, Jellyfish removal** | layered backgrounds, day and night from the clock, floor items tied to dirt; new or replaced assets reach existing badges through `copy_assets.py --update` from plan 1 task 15 (extend it for replacements); updates guide ch 3 | 1 (dirt), 2 (screen layout) | 14 (not written by this planner; read it before planning) | M |
| 8 | Long-term layer | care score display, growth forms as titles, traits as modifiers, Unicorn Realm record fields, demo pet's occasional Only pets hobby moment (spec 8.3), as a scheduled demo timer (`demo::timers`), never a per-check dice roll; updates guide ch 12 | 1, 2 | 4, 17 row 11 | S to M |
| 9 | **New-state sprites and throwing up** | one frame per pet for 12 states (tipsy, wasted, hung over, sore, injured, groggy, stuffed, thirsty, wired, throwing up, needs rehab, starving) for six pets (Bartholomeus, Cat, Slug, Panda, Badger, Starfish); Relaxing `0E` for Relax; Sport on a stuffed pet throws up ("Bleurgh!"); plan 7's five floor-litter items redrawn in the pets' style, larger, baked into flash (92 B). Part A (art from the pet's own frames, owner gallery gate, at most 3 rounds) runs now; Part B (anim ids `0x29` to `0x34`, stand-in fallback, the throw-up dose, 72 PCX within the computed space limit, 21 files retired: the Drained art and Relax's third frame) after 5 and 7 are merged; updates guide ch 5 (ch 3 floor shot redrawn) | Part A: none; Part B: 5 (`exercise` split, `fight_over`), 7 (floor lines, `background.rs` floor slots and tests, `copy_assets` changes, space guard) | `BORNPETS_V3_ART_DESIGN.md` (owner decisions 2026-11-14); plan `BORNPETS_V3_PLAN_9_ART.md` | M |
| G | **Player's guide** | A5 Amiga-style PDF (Typst) from `guide/`: story, controls, every on-screen word, numbers from code, simulator screens; each later plan updates its chapter | 1, 2 (describes the badge as built) | `BORNPETS_V3_GUIDE_DESIGN.md` (approved 2026-11-14) | M |

Every later plan runs `make guide` and fixes what the check flags.

Plans 3, 4, 5 and 7 are independent of each other once 1 and 2 are in. 6 needs the work action from 1 and screens from 2. Suggested sequence for a festival deadline: 1, 2, 4, 3, 6, 5, 7, 8.

Owner order (2026-11-14): plans 1 and 2 are done (merged at `1a67963`). Stages run two at a time in their own worktrees and `ranzbak/bornpets-v3-*` branches: guide (G) with 3 first, then 5, 6, 7 and 8 in pairs, and plan 4 (NFC stations) last.

Owner (2026-11-14): plan 6b is **dropped**. Plan 6 (hard mode and money) is kept and will run paired with plan 7 or plan 8. Plan 5 (battles) is planned in `BORNPETS_V3_PLAN_5_BATTLES.md`.

## Tunable balance across plans

Plan 1 makes every v3 value a `Thresholds` field with a Classic and a Casual value and a key in `BPETS_V3.CFG` (owner, 2026-11-13). Every later plan follows the same rule for the values it adds (demo policy, station limits, battle stats, prices, hard-mode amounts) and regenerates the commented template `assets/to-badge/BPETS_V3.CFG`. The v3 firmware never touches the old `BORNPETS.CFG` (the old firmware keeps using it), and an asset update never overwrites an existing `BPETS_V3.CFG`.

## Save format across plans

Plan 1 defines save v3 once, with 32 reserved zero bytes, so plans 3 (demo timers, bytes 149 to 159) and 6 (hard-mode counters, 17 bytes, 160 to 176) fill reserved bytes without another version bump. Plan 5 uses none of the tail: its 8 bytes (demo record, prize and decline timers) go into format-2 head slots that format 3 writes as zero (offsets 38, 42, 44, 46), and its per-pair cooldown lives in the friends list (new kv key `friends/list_v3`). Plan 1 owns the save format 2 to 3 upgrade.

## Are the old "phase 1" tuning fixes still worth shipping before v3?

Mostly no. They patch the v1 engine that plan 1 replaces: the softer sick rates, the Only-pets change and the mini-game reward conditions would be thrown away.

Worth doing only if a firmware release goes out before plan 1 lands, because each is a few lines and fixes a real exploit or a visible bug:

1. Aspirine cooldown no longer suppresses the diabetes and alcoholism penalties (`src/game/engine/mod.rs:983` and `:1062`).
2. The wrong toast on Battle (`src/game/modal.rs:425`).
3. The doc corrections of the audit's section 5 (`GAME.md`, `GAMES.md`, `USER_GAMES.md`), which stay true until v3 replaces those docs.

Skip the rest.

## Not planned yet

- Catch-up of at most 12 h on boot (17 row 30) needs the watch clock at game init; it belongs to plan 1's `lifecycle::try_load` but depends on how the RTC survives power-off, which the spec does not settle. It is a task in plan 1 guarded on "clock known".

- **Final task, after all stages (including plan 4) and the final regression:** rewrite GAME.md for v3 as the local plain-text companion of the PDF guide (controls, quick start, short rules, music, de-ghost, debug; links the PDF), and add Music, screen de-ghost and Traits (if still in v3) to the guide.
