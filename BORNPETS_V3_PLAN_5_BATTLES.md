# BornPets v3, plan 5: battles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a fight is a test of the pet's whole condition, here and now. The challenger sends a CHALLENGE with its snapshot. The other badge buzzes, takes its own snapshot and shows a pre-battle screen with both pets' numbers and the exact chance to win: Fight or Decline. Both badges then run the same fight from the same seed and pay the same costs (energy, strain, bruises, mood). With no answer in 20 seconds the challenger fights the last beacon. The first win of the day pays 100 HAX, later wins 20. Injured, drunk, weak and sleeping pets are refused with the true reason; there is no age limit (owner). Demo pets fight only demo pets. A hibernating pet answers, but the fight does not touch it.

**Architecture:** three layers, each testable on the host. (1) `src/game/engine/combat.rs`, pure integer math: `Body` (the pet as percentages) → `Snapshot` (six numbers and 20 condition flags), `fight`, `win_chance`, `form`, `factors`. It is a port of `prototypes/bornpets-simv3/src/battle.rs`. (2) The engine (`GameState`): `battle_refusal`, `fight_over` (costs, records, prize), the decline penalty as a solver timer, 8 save bytes in dead head slots. (3) `src/game/battle.rs`, a pure protocol `Session` that takes a `&mut GameState` and returns packets and UI effects, so two badges can be wired together in a test. Thin glue (`battle_link.rs`, the mesh dispatch, a one-shot firmware timer, the beacon ticker) connects it to the singleton game, the radio and the screen. No polling anywhere: the 20-second window is one embassy timer per challenge, and the engine sees a fight as one event, like `friend_boost`.

**Tech stack:** Rust, `no_std` firmware (embassy, embedded-graphics; FONT_7X13, FONT_7X13_BOLD and the already-linked FONT_6X10), MeshCore SHDW group data, host tests through the `simulator` feature, Typst guide.

**Spec:** `BORNPETS_V3_DESIGN.md` (repo root): 12 (all of it, including 12.8), 15.7 (the current formula), 17 rows 16 to 23a (owner decisions), 18.4 (current matrix and lifestyles), 18.5 (money with 100 / 20 HAX), 18.8 (round 2: the 100 / 20 rule), 18.9 ("one fight a day pays about two shifts: Accepted"), 8.5 (beacon demo flag). Prototype: `prototypes/bornpets-simv3/src/battle.rs` (`derive`, `fight`, `builds`, `matrix`) and `main.rs` 522 to 538 (`battle_cost`). Roadmap row 5: `BORNPETS_V3_ROADMAP.md`. Rulings R1 to R26: `../BornHack-Cyberegg-v2/.superpowers/sdd/BORNPETS_V3_PLAN_1_ENGINE/global-constraints.md`. Plan-3 constraints: `../BornHack-Cyberegg-v3demo/.superpowers/sdd/BORNPETS_V3_PLAN_3_DEMO/global-constraints.md`. Guide width budgets: `BORNPETS_V3_PLAN_G_GUIDE.md` Task 14. Guide-stage bug list: `../BornHack-Cyberegg-v3guide/.superpowers/sdd/BORNPETS_V3_PLAN_G_GUIDE/task-11-report.md` items 1, 4, 5. Format exemplar: `BORNPETS_V3_PLAN_3_DEMO.md`.

**Code base:** worktree `BornHack-Cyberegg-v3battles`, branch `ranzbak/bornpets-v3-battles`, HEAD `5e59574` (plans 1, 2, G and 3 merged). Anchors at that commit:
- `src/game/battle.rs` (576 lines): `CombatStats` 31, `derive_combat_stats` 51, `simulate` 113, `PET_BATTLE_TYPE` 165, `BattleResultMsg` 167, `broadcast_result` 252, `challenge` 285, `on_battle_result` 351.
- `src/game/battle_view.rs`: `words` 37, picker messages 68 to 82, `RESULT` 90, `try_challenge` 353, `handle_input` 392, `picker_title` 436, `draw_picker` 443, `draw_result` 514 (reward line 583 to 597).
- `src/game/friends.rs`: `PetBeacon` 33 to 80 (`BEACON_SIZE` 22), `FriendRecord` 89 to 200 (`FRIEND_RECORD_SIZE` 36), `FriendsList` 204, `rebase_after_reboot` 330, `init` 386, `save_if_needed` 405, `record_battle_vs` 433, `on_pet_beacon` 488.
- `src/game/engine/mod.rs`: `BATTLE_HAX_REWARD` 130, `GameState` 244 (`wins` 348, `cooldown_battle` 375), `wake` 1092, `sync_clock` 1191, `refusal_for` 1233, `game_refusal` 1256, `cannot_act` 1432, `PetStats` 1555 (`can_battle` 1635), `trait_pct` 1700, `stats` 1716 (`can_battle` 1786), `friend_boost` 1868, `record_battle` 1876, `add_money` 1893, `SAVE_SIZE` 1939, `to_bytes` 2066 (dead slots written as 0 at 2091 to 2101), `from_bytes` 2166, `from_bytes_v2` 2229, `Head` 2269.
- `src/game/engine/doses.rs`: `Refusal` 17, `Reason` 30, `Refusal::text` 45, `Reason::pack` 80, `unpack` 101, `exercise` 288 (injury 339 to 349). `causes.rs`: `StrainWhy` 11 (texts 32 to 37), `Cause` ~82, `pack` 190 (last code 15). `regime.rs`: `mood_penalty` 260 (craving 266 to 271). `solver.rs`: `timer_bound` 191, `feeds` 228 to 243. `needs.rs`: `SORE_PCT` 32. `thresholds.rs`: `BATTLE_COOLDOWN` 92, Classic 420, Casual 746, demo `validate` 1967.
- `src/game/lifecycle.rs`: `now_tick` 42, `cycle` 410, `is_demo` 486, `toggle_demo` 493, `alert_due` 661, `game_refusal` 825, `friend_boost` 904, `wake` 962, `battle_record` 1081, `combat_stats` 1091, `record_battle` 1104, `with_game_now` 1173.
- `src/game/mod.rs`: `Toast::BattleWon/BattleLost` 95 and 182, `show_toast` 369, `show_battle_anim` 311, `next_ui_deadline_tick` 593.
- `src/game/modal.rs`: `Item::Battle` 193, `available` 398, `reason` 498, activate 554, `row_text` 904.
- `src/bin/embassy.rs`: `pet_beacon_ticker_task` 1669. `src/fw/mesh/meshcore.rs`: SHDW dispatch 862 to 874.
- `src/guide/words.rs` 218 (social words), `src/guide/data.rs` 18 and 172 (`BATTLE_HAX_REWARD`), 57 (`BATTLE_COOLDOWN` override), `unit_of` ~75. `guide/chapters/11-friends-battles-stations.typ` (Battles section), `05-care-actions.typ` 576 (the never-raised toasts), `09-money-and-hax.typ` 96.
- Tests: `tests/save_format.rs` 136 to 139 (dead slots are zero), 607 (beacon golden), `tests/guide_words.rs` 86 (drawn literals), `.github/workflows/tests.yml` matrix 28, `others` 43 to 47.

## Global Constraints

Every task's requirements include this section.

**Owner rules (verbatim where quoted):**
- "the delta engine as it's written now needs to stay the algorithm to drive progress". No fixed-step loop and no periodic polling. A fight is one engine event (`fight_over`, like `friend_boost`). Every timer that a mood or health term reads is a solver boundary (`solver::timer_bound` `feeds`). The 20-second answer window is a one-shot embassy timer per challenge, not a poll. A step loop exists only as the simulator test oracle (`Driver::Oracle`).
- **TDD, at quality.** Write the failing test first, see it fail, then the code. Implementers are cheaper models: the plan carries the design; where it leaves a choice, take the simplest one that passes the tests and say so in the report.
- **Tests are not free (owner, 2026-11-14).** Each task runs **only its own focused tests** locally. No task pushes. The full suite, both firmware builds, the flash gate and the long simulations happen once, in Task 12 (regression), on the GitHub mirror. Task 13 fixes what Task 12 finds.
- Local commands use 2 cores, one cargo command at a time: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo ... -j 2` (another stage runs in parallel on cores 0 and 1; the controller may swap the pair, never widen it). Never run `cargo fmt` on the whole tree. Never use `git stash`.
- **CI and pushes.** CI runs on the GitHub mirror only; every workflow job keeps `if: github.server_url == 'https://github.com'`. Never use Codeberg CI. Push `ranzbak/bornpets-v3-battles` to the `github` remote only (public mirror `ranzbak/bornhack-firmware-2026-ce`), and only in Tasks 12 and 13. SSH git commands are prefixed `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh`.
- **Saves.** "Never write the old kv key `state`, `game/name` or `BORNPETS.CFG`. The save is `state_v3`." Version byte 3 and `SAVE_SIZE` 177 stay. Bytes 149 to 159 are demo mode's. Bytes 160 to 176 (17 bytes) stay reserved and zero for plan 6. Battles use **8 bytes of the format-2 head slots that format 3 already writes as zero** (offsets 38, 42, 44, 46: the old `cooldown_feed`, `cooldown_relax`, `cooldown_play`, `cooldown_exercise`). The friends list moves to a new kv key `friends/list_v3`; the old `friends/list` is read once for the upgrade and never written, so the old firmware keeps its friends.
- **Balance values** are `Thresholds` fields with a Classic and a Casual value and a `BPETS_V3.CFG` key (the `BATTLE_*` table in Task 1). The template `assets/to-badge/BPETS_V3.CFG` is regenerated with `BPETS_REGEN=1` in the task that adds a key, and the `USER_GAMES.md` "Custom balance" key list is updated in the same commit. Casual = Classic except health damage halved (Task 1 table). **Exception, the wire contract:** the fight itself (rounds, hit chance, damage, the chance seeds) is `const` in `combat.rs`. Both badges compute the same fight from the same two snapshots; a per-badge config there would make them disagree. Snapshot formula, costs, refusals, prizes and cooldowns are per-badge and tunable.
- **Labels and refusals.** "If the labels in the game need to be changed to make the game flow more clear to the player, do so", within the width budgets below. Every disabled or refused action shows its true reason from the one mapping (R24: `doses::Reason::text`). A refused CHALLENGE tells the challenger why (the DECLINE carries the reason).
- **Demo mode** (spec 12.5, plan 3): demo pets fight only demo pets; both badges check; the beacon carries the demo bit; demo fights go to a separate demo record; HAX stays frozen; the demo floors hold (`demo::floor` on every battle health loss); an unattended demo pet auto-accepts a demo challenge; a demo pet never starts a fight. Demo mode is fully silent: no buzz for an incoming challenge.
- **Flash.** The SSD1680 release image is 866 KiB of the 960 KiB gate. This plan's estimate is in the Self-review. If the image goes over, the owner allows removing the Lights Out mini-game (Task 13, its own commit). No new font, no new sprite.
- **Guide.** Every new on-screen word is in a `words()` list the guide collector reads, and the guide marks and explains it with `#w(...)`; `make guide` must pass (Task 11).
- Commit footer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**Carried from plans 1 to 3:** integer math only in `src/game/engine/` (except `sim.rs`); no `alloc` in engine code or in `battle.rs` (heapless only); levels are `u16` with `P = 655` per percent; 1 engine minute = 6 ticks, 1 tick = 10 s; engine wake cap 720 ticks; R15 (code this plan makes dead is removed in the same commit, no commented-out code); R17 and R20 (split invariance, timers that feed mood bound the segment at `remaining - 1`); R24; R26 ("BornPets v3"); R7 and R13 (the known failures `name_screen::tests::drops_control_and_beyond_latin1` and the `signed_channel.rs` doctest are not yours). Buzz only for danger, the alerting status words and (new) an incoming challenge, through the one gate `lifecycle::alert_due`, never in demo mode.

**Width budgets** (FONT_7X13 is 7 px per column, 21 columns on 152 px; FONT_6X10 is 6 px per column, 25 columns):

| Where | Budget |
|---|---|
| Reason text (`Reason::text`, `heapless::String<16>`) | ≤ 16 columns |
| Menu row (Play menu "Battle: <form>", wait suffix included) | ≤ 17 columns |
| Toast line | ≤ 15 columns |
| Full-screen line, FONT_7X13 (picker rows, result card, picker messages) | ≤ 21 columns |
| Full-screen line, FONT_6X10 (pre-battle screen, picker foot) | ≤ 25 columns |
| Pre-battle half line, FONT_6X10 (one side's name or factor line) | ≤ 12 columns |
| Cause `what` / `why` | ≤ 12 / ≤ 15 columns |

Each text table gets a worst-case width test in the task that adds it.

**Verify (V)**, the last step of Tasks 1 to 11:
1. Run the task's focused tests (the commands are in the task). Nothing else.
2. `git status --short` shows only the files the task names.
3. No push. Report the test names run and their result.

## Review Focus

The failure modes most likely to reach a player unnoticed, most likely first. Each is pinned by a test.

1. **The two badges disagree about a fight.** Different seeds, a snapshot encoded differently from how it is decoded, a fight that reads a per-badge config value, or the target counting a fight twice (live, then a late RESULT). Pinned in Task 2 by `fight_reads_no_config` and `seed_is_symmetric_in_roles`, in Task 5 by `snapshot_wire_round_trips_every_field`, and in Task 7 by `live_fight_agrees_on_both_badges`, `chances_add_up_to_100` and `late_result_after_accept_is_ignored`.
2. **A pet fights or pays when it must not.** An injured, drunk, weak, sleeping-challenger, hibernating or other-league pet gets through, the target of a fallback fight is paid (today's bug), or both sides are paid. Pinned in Task 3 by `refusals_follow_the_list`, `injured_pet_cannot_battle`, `fallback_target_is_never_paid`, in Task 7 by `exactly_one_side_is_paid`, `other_league_is_declined_silently`, `hibernating_target_is_untouched`.
3. **A fight breaks the engine's time.** The decline penalty is a mood term on a timer: if the solver does not bound it, the badge sleeps past the mood change, or one long update differs from a cut one. Pinned in Task 4 by `decline_penalty_ends_on_time`, `decline_wake_is_exact_on_random_states` and `decline_is_split_invariant`.
4. **Saves and the friends list break.** A plan-3 save must load with zero battle fields, the v2 upgrade must not read the old cooldowns into them, the plan-6 tail must stay zero, the old `friends/list` must migrate and never be written. Pinned in Task 3 by `plan3_save_loads_with_empty_battle_slots`, `v2_upgrade_leaves_battle_slots_zero`, `battle_fields_round_trip`, `tail_stays_zero_for_plan_6`, and in Task 6 by `old_friends_blob_migrates`, `never_writes_the_old_friends_key`.
5. **Words that lie or overflow.** A refusal with no reason, a factor line that does not fit half the screen, a toast that is never raised (today's bug), the estimate shown as exact. Pinned in Task 3 by `battle_reasons_fit`, Task 2 by `factor_words_fit_half_a_line`, Task 7 by `heard_result_raises_the_toast`, Task 9 by `picker_row_marks_the_estimate` and the width tests.

## File map

| File | Change |
|---|---|
| `src/game/engine/combat.rs` | create: `Body`, `Snapshot`, `Flags`, `Factor`, `derive`, `fight`, `Outcome`, `seed`, `chance_seed`, `win_chance`, `Form`, `form`, `factors`, `Decisive`, `decisive`, wire ranges |
| `src/game/engine/mod.rs` | `pub mod combat`; fields `demo_wins`, `demo_losses`, `prize_min`, `decline_min`; `battle_refusal`, `Fought`, `fight_over`, `record_only`, `declined`, `battle_record`, `snapshot`; save slots 38/42/44/46; `record_battle` and `BATTLE_HAX_REWARD` removed; `PetStats::can_battle` from `battle_refusal` |
| `src/game/engine/doses.rs` | `Refusal::{TooWeak, TooDrunk, DemoOnly, NoDemo}`; `battle` costs; the injury block shared with `exercise` (`strain_up`) |
| `src/game/engine/causes.rs` | `StrainWhy::Fight` |
| `src/game/engine/regime.rs` | `craving(s)` extracted; decline mood term in `mood_penalty` |
| `src/game/engine/solver.rs` | `decline_min` in `feeds` |
| `src/game/engine/advance.rs` | `prize_min` and `decline_min` count down |
| `src/game/engine/thresholds.rs` | the `BATTLE_*` fields, Classic and Casual values, keys, `validate` rules; `BATTLE_COOLDOWN` 12 → 180 |
| `assets/to-badge/BPETS_V3.CFG` | regenerated |
| `src/game/engine/sim.rs` | `Act::Battle`, routines `fighter`, `fighter_six`, `brawler`, `brawler_six`, `sumo`, battle `Outcome` fields |
| `src/game/battle.rs` | rewrite: wire messages (`ChallengeMsg`, `AcceptMsg`, `DeclineMsg`, `BattleResultMsg` kept), `Session` state machine, `Out`, `PreBattle`; old `CombatStats`, `derive_combat_stats`, `simulate`, `challenge` removed |
| `src/game/battle_link.rs` | create: the singleton `Session`, `lifecycle` and `friends` glue, `tx` of packets, `on_*` entry points for the mesh, `deadline` hand-off |
| `src/game/friends.rs` | `PetBeacon` v3 (25 bytes); `FriendRecord` v3 (43 bytes: stamina, focus, flags, `fought_tick`); kv `list_v3` with the `list` upgrade; `league`, `estimate_snapshot`, `pair_wait_secs` |
| `src/game/battle_view.rs` | picker rows, picker foot, pre-battle screen, waiting and declined notes, result card; `words()` |
| `src/game/stat_bar.rs` | `draw_mirrored_bar` |
| `src/game/modal.rs` | Battle row label with the form word; reason from `battle_refusal` |
| `src/game/lifecycle.rs` | `battle_refusal`, `fight_over`, `record_only`, `declined`, `snapshot`, `battle_form`; `combat_stats`, `record_battle` removed; `toggle_demo` cancels a challenge; `alert_due` call for a challenge |
| `src/game/mod.rs` | `pub mod battle_link`; `next_ui_deadline_tick` unchanged (the window is a firmware timer); battle full-screen view routing |
| `src/fw/mesh/meshcore.rs` | dispatch `PET_CHALLENGE_TYPE`, `PET_ACCEPT_TYPE`, `PET_DECLINE_TYPE` |
| `src/bin/embassy.rs` | beacon ticker sends the v3 beacon; `battle_timer_task` |
| `src/guide/data.rs`, `src/guide/words.rs`, `src/guide/shots.rs` | units for `_PTS` / `_HAX`; `BATTLE_HAX_REWARD` removed; words; shot `pre-battle` |
| `guide/chapters/11-friends-battles-stations.typ`, `05-care-actions.typ`, `09-money-and-hax.typ` | battles rewritten; toast list; prize row |
| `tests/battle_v3.rs` | create: combat, engine, protocol and balance tests; long runs `#[ignore]` |
| `tests/save_format.rs` | dead-slot test narrowed; v3 beacon and friends goldens |
| `tests/engine_v3.rs` | template regeneration only (hook exists) |
| `.github/workflows/tests.yml` | matrix suite `battle_v3` with `--include-ignored --nocapture`; excluded from `others` |
| `GAME.md`, `USER_GAMES.md` | Battles section; key list |

---

### Task 1: the battle snapshot (spec 15.7, 18.4)

The pet's condition becomes six fighting numbers and a set of condition flags. A port of the prototype's `derive`, with the bands read from the need-band accessors so Casual and custom bands move the fight with them (18.1 row 3).

**Files:**
- Create: `src/game/engine/combat.rs` (this task: `Body`, `Snapshot`, `Flags`, `Factor`, `derive`, `Body::of`, `Body::REFERENCE`, `WIRE` ranges), `tests/battle_v3.rs`.
- Modify: `src/game/engine/mod.rs` (`pub mod combat;`, `GameState::snapshot`), `src/game/engine/regime.rs` (`craving` extracted, `mood_penalty` calls it), `src/game/engine/thresholds.rs`, `assets/to-badge/BPETS_V3.CFG`, `USER_GAMES.md`, `src/guide/data.rs` (`unit_of`: `_PTS` → `Unit::Count`, `_HAX` → `Unit::Hax`, if not there yet).

**Interfaces:**
- Consumes: nothing new.
- Produces:
```rust
// combat.rs
/// The pet as the fight sees it, every field in whole percent (traits 0..=100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    pub fit: i32, pub energy: i32, pub booze: i32, pub mood: i32, pub health: i32,
    pub strain: i32, pub full: i32, pub water: i32, pub fun: i32, pub dirt: i32, pub weight: i32,
    pub vit: i32, pub cur: i32, pub res: i32,
    pub rested: bool, pub injured: bool, pub groggy: bool, pub hangover: bool,
    pub craving: bool, pub deprived: bool, pub demo: bool,
}
impl Body {
    /// The prototype's average pet (`Cond::BASE`): fit 60, energy 70, booze 0, mood 75, health 85,
    /// strain 0, full 55, water 60, fun 50, dirt 30, weight 50, traits 50, no flags.
    pub const REFERENCE: Body;
    pub fn of(s: &GameState) -> Body; // needs::pct of each level, trait_pct, flag timers > 0, regime::craving, s.demo.on
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Flags(pub u32);   // bit = Factor as u8 (0..=19); bit 22 Star (reserved, always 0, plan 8); bit 23 demo
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Factor {          // declaration order is display priority within each sign
    Rested, Heavy, Tipsy, Cheerful,                                   // plus (helps)
    Injured, Sloppy, HungOver, TooLight, Hungry, Thirsty, Craving, Wired,
    Stuffed, Bored, Down, Groggy, WornOut, Waterlogged, Sore, Filthy, // minus (hurts)
}
impl Factor { pub const ALL: [Factor; 20]; pub fn helps(self) -> bool; pub fn text(self) -> &'static str; }
impl Flags { pub fn has(self, f: Factor) -> bool; pub fn demo(self) -> bool; pub fn with(self, f: Factor) -> Flags; pub fn with_demo(self, on: bool) -> Flags; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot { pub power: u8, pub guard: u8, pub speed: u8, pub hp: u8, pub stamina: u8, pub focus: i8, pub flags: Flags }
/// Inclusive wire ranges; `derive` clamps into them, a received snapshot outside them is refused (Task 5).
pub const WIRE_POWER: (u8, u8) = (5, 70);
pub const WIRE_GUARD: (u8, u8) = (0, 40);
pub const WIRE_SPEED: (u8, u8) = (5, 100);
pub const WIRE_HP: (u8, u8) = (15, 120);
pub const WIRE_STAMINA: (u8, u8) = (0, 10);
pub const WIRE_FOCUS: (i8, i8) = (-100, 20);
pub fn derive(b: &Body) -> Snapshot;
// mod.rs
pub fn snapshot(&self) -> combat::Snapshot; // derive(&Body::of(self))
// regime.rs
pub fn craving(s: &GameState) -> bool; // the existing rule: dependent, booze < MOOD_CRAVING_BOOZE_PCT, awake, no rehab
```
- `derive` is `prototypes/bornpets-simv3/src/battle.rs` `derive` line by line, with Rust `/` (truncating) exactly as the prototype, and these substitutions: the literal bands become accessors in percent (`78` `FULL_HI_PCT`, `83` `WATER_HI_PCT`, `38` `FULL_LO_PCT` / `WATER_LO_PCT` / `WEIGHT_LO_PCT`, `33` / `73` `FUN_LO_PCT` / `FUN_HI_PCT`, `58` `DIRT_HI_PCT`, `63` `WEIGHT_HI_PCT`, strain `40` `needs::SORE_PCT`); the magnitudes become the keys below. `load` is `booze`; `felt = min(100, booze * (BATTLE_FELT_BASE_PCT - weight) / 100)`. After the prototype's last line, clamp each number into its `WIRE_*` range. The craving and deprived inputs are `Body` flags, the prototype's `stim` is `fun`.
- Flags, set from the same `Body`: `Rested` rested; `Heavy` weight > weight hi; `Tipsy` courage > 0 and felt ≤ `BATTLE_SLOPPY_PCT`; `Cheerful` mood ≥ `BATTLE_HIGH_MOOD_PCT`; `Injured`; `Sloppy` felt > `BATTLE_SLOPPY_PCT`; `HungOver`; `TooLight` weight < weight lo; `Hungry` full < full lo; `Thirsty` water < water lo; `Craving`; `Wired` fun > fun hi; `Stuffed` full > full hi; `Bored` fun < fun lo; `Down` mood < `BATTLE_LOW_MOOD_PCT`; `Groggy`; `WornOut` deprived; `Waterlogged` water > water hi; `Sore` strain > `SORE_PCT`; `Filthy` dirt > dirt hi; bit 23 = `demo`.
- `Factor::text` (on-screen words, ≤ 11 columns so `+word` fits half a line): `well rested`, `heavy`, `tipsy`, `cheerful`, `injured`, `sloppy`, `hung over`, `too light`, `hungry`, `thirsty`, `craving`, `wired`, `stuffed`, `bored`, `down`, `groggy`, `worn out`, `waterlogged`, `sore`, `filthy`.

**Tunables** (Classic / Casual; `_PCT` fields `u8` with `clamp_pct`, `_PTS` fields `u8` with `v.min(100)`, `_MIN` `u16`, `_HAX` `u16`; all read through the generated accessors):

| Key | Classic | Casual | Meaning (prototype term) |
|---|---|---|---|
| `BATTLE_POWER_BASE_PTS` | 22 | 22 | power base |
| `BATTLE_POWER_WEIGHT_PCT` | 28 | 28 | power per weight |
| `BATTLE_COURAGE_PEAK_PTS` | 8 | 8 | drink courage at its peak (felt 30) |
| `BATTLE_HIGH_MOOD_PCT` / `_PTS` | 80 / 3 | same | mood at or above: power +3, `cheerful` |
| `BATTLE_LOW_MOOD_PCT` | 40 | 40 | mood below: power and focus down, `down` |
| `BATTLE_GUARD_WEIGHT_PCT` | 26 | 26 | guard per weight |
| `BATTLE_GUARD_BASE_PTS` | 7 | 7 | subtracted from guard |
| `BATTLE_SPEED_BASE_PTS` | 25 | 25 | speed base |
| `BATTLE_SPEED_FIT_PCT` | 40 | 40 | speed per fitness |
| `BATTLE_RESTED_SPEED_PTS` / `_FOCUS_PTS` | 5 / 5 | same | well rested |
| `BATTLE_GROGGY_SPEED_PTS` / `_FOCUS_PTS` | 10 / 5 | same | groggy |
| `BATTLE_HANGOVER_SPEED_PTS` / `_FOCUS_PTS` | 8 / 12 | same | hung over (stamina −1 stays structure) |
| `BATTLE_CRAVING_FOCUS_PTS` / `_POWER_PTS` | 10 / 3 | same | dependent and sober |
| `BATTLE_HEAVY_UNFIT_FIT_PCT` | 50 | 50 | heavy and fitness below: stamina −1 more |
| `BATTLE_SLOPPY_PCT` | 40 | 40 | felt drink above: sloppy |
| `BATTLE_FELT_BASE_PCT` | 150 | 150 | `felt = booze * (this − weight) / 100` |
| `BATTLE_HP_BASE_PTS` | 45 | 45 | HP base |
| `BATTLE_HP_WEIGHT_PCT` | 25 | 25 | HP per weight |
| `BATTLE_POWER_FLOOR_PTS` | 33 | 33 | 17 row 23 floors |
| `BATTLE_GUARD_FLOOR_PTS` | 14 | 14 | |
| `BATTLE_HP_FLOOR_PTS` | 60 | 60 | |
| `BATTLE_STAMINA_FLOOR_PTS` | 1 | 1 | 17 row 18 |
| `BATTLE_INJURED_PCT` | 60 | 60 | injured: power and speed times this |

The slopes and divisors of the formula (`/3`, `*3/5`, `/2600`, `/25`, `/15`, `*5/4`, ...) are structure, not balance, and stay literals with a comment, like the 3/2 and 1/2 weights in `regime.rs` (plan 1 precedent). Task 3 adds the cost, refusal and prize keys; Task 2 the form keys.

- [ ] **Step 1: write the failing tests** in `tests/battle_v3.rs`, `mod snapshot`:
```rust
/// The prototype builds (`battle::builds`) as `Body`, traits 50.
fn builds() -> [(&'static str, Body); 20] { /* the 20 rows of prototypes/bornpets-simv3/src/battle.rs `builds`, load -> booze, stim -> fun, hyd -> water */ }
#[test] fn builds_match_section_18_4() {
    // (power, guard, speed, hp, stamina, focus), Classic preset, from `cargo run -- battle` at 5e59574:
    let want = [
        (39,16,71,75,4,0), (43,15,71,75,4,-2), (33,14,75,70,4,0), (47,23,49,80,3,0), (52,21,49,80,3,-3),
        (39,16,70,72,6,5), (36,16,59,68,3,0), (45,25,20,60,1,0), (52,22,20,60,1,-4), (33,14,48,60,2,0),
        (36,14,59,68,3,-48), (33,16,59,68,3,-10), (36,16,59,68,3,-19), (36,16,59,68,3,-12), (36,16,59,68,1,0),
        (36,16,43,68,3,0), (36,16,59,68,2,-4), (36,16,59,68,1,0), (36,16,51,68,1,-14), (20,16,35,68,3,0),
    ];
    // assert_eq! each build's six numbers, with the build name in the message
}
#[test] fn flags_name_the_conditions() {
    // "sumo, two beers": Heavy and Tipsy, not Sloppy; "wasted": Sloppy; "hung over": HungOver, Thirsty;
    // "injured": Injured, Sore; "well rested": Rested; "average": no factor bit; demo body: flags.demo()
}
#[test] fn snapshot_stays_in_wire_ranges() {
    // 20_000 seeded random Bodies (every field 0..=100, every flag random) plus the 2^7 flag corners
    // at fields 0 and 100: every derive() field inside its WIRE_* range
}
#[test] fn snapshot_of_a_pet_reads_its_state() {
    // GameState::new_egg(7, PetKind(0)) made Active, set weight 80 %, fitness 80 %, booze 20 %:
    // s.snapshot() == derive(&Body::of(&s)); Body::of(&s).weight == 80, .booze == 20
}
#[test] fn bands_move_the_fight() {
    // Casual installed in a separate test binary is not needed: build Body with full 36 and check
    // Hungry under Classic (FULL_LO 38); the accessor, not a literal, decides (read the source:
    // include_str!("../src/game/engine/combat.rs") contains no " 78" / " 83" / " 63" literal)
}
#[test] fn factor_words_are_listed_once() { /* Factor::ALL texts unique; helps() true exactly for the first 4 */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 snapshot::` fails (`combat` missing).
- [ ] **Step 3: implement** `combat.rs` (this part), `craving`, `GameState::snapshot`, the keys and setters, `unit_of`. Regenerate the template: `BPETS_REGEN=1 CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test engine_v3 template_`. Add the new keys to the `USER_GAMES.md` key list (Classic and Casual).
- [ ] **Step 4: run them and see them pass.** `... --test battle_v3 snapshot::`, `... --test engine_v3 template_`, `... --test guide_data`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): v3 battle snapshot and its tunables`.
- [ ] **Step 6: V.**

### Task 2: the fight, the chance, the form (spec 12.2 "The fight", 12.4 "The chance", 12.6)

**Files:**
- Modify: `src/game/engine/combat.rs`, `src/game/engine/thresholds.rs` (three form keys), `assets/to-badge/BPETS_V3.CFG`, `USER_GAMES.md`.
- Test: `tests/battle_v3.rs` `mod fight`.

**Interfaces:**
- Consumes: Task 1 `Snapshot`, `Body`, `derive`, `Factor`.
- Produces:
```rust
// combat.rs — the wire contract: consts, never config
pub const ROUNDS: u8 = 10;
pub const HIT_BASE: i32 = 72;          // hit% = clamp(72 + (speedA' - speedD')/3 + focusA, 15, 95)
pub const HIT_MIN: i32 = 15; pub const HIT_MAX: i32 = 95;
pub const TIRED_PCT: i32 = 60;         // past its stamina a pet strikes at 60 % speed and power
pub const DAMAGE_MIN: i32 = 3;
pub const CHANCE_FIGHTS: u32 = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome { pub a_won: bool, pub a_hp_pct: u8, pub b_hp_pct: u8, pub rounds: u8 }
/// `a` is the challenger. Prototype `fight` exactly, with `platform::xorshift32` for `rng`.
pub fn fight(seed: u32, a: &Snapshot, b: &Snapshot) -> Outcome;
/// Seed of a live fight: both badges compute it from the same four values.
pub fn seed(nonce_a: u16, nonce_b: u16, id_a: [u8; 2], id_b: [u8; 2]) -> u32;
pub fn chance_seed(k: u32) -> u32;     // k.wrapping_mul(2_654_435_761).wrapping_add(7)
/// The challenger's chance in percent, rounded to the nearest 5 (ties up): 64 fights on chance_seed(0..64).
pub fn win_chance(a: &Snapshot, b: &Snapshot) -> u8;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form { Top, In, Off, Poor }
impl Form { pub fn text(self) -> &'static str; }   // "top form", "in form", "off form", "poor form"
/// The pet's own chance against `derive(&Body::REFERENCE)` as `a`.
pub fn form(own: &Snapshot) -> Form;
/// Up to two factor lines to show for one side: the first helping and the first hurting factor
/// in `Factor` order; with none of one sign, the first two of the other.
pub fn factors(s: &Snapshot) -> [Option<Factor>; 2];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decisive { Factor(Factor), OutOfSteam(u8) }
/// Why the loser lost, if one thing stands out: its first hurting factor; else, if it fought past
/// its stamina (`outcome.rounds > loser.stamina`), OutOfSteam(loser.stamina + 1); else None.
/// Returns (loser_is_a, why).
pub fn decisive(o: &Outcome, a: &Snapshot, b: &Snapshot) -> Option<(bool, Decisive)>;
```
- `seed`: `let ids = (u16::from_le_bytes(id_a) as u32) << 16 | u16::from_le_bytes(id_b) as u32; let x = ((nonce_a as u32) << 16 | nonce_b as u32) ^ ids ^ 0x9E37_79B9; xorshift32(xorshift32(x))` (no `| 1`: `fight` applies `seed | 1` itself; controller ruling at Task 2 review). A fallback fight uses `nonce_b = 0`.
- `win_chance`: `w` = wins of `a` over the 64 seeds; `((w * 100 + 32) / 64 + 2) / 5 * 5`. The target shows `100 - win_chance(a, b)`, so both badges show figures that add up to 100.
- Form keys (Classic / Casual): `BATTLE_FORM_TOP_PCT` 65 / 65, `BATTLE_FORM_IN_PCT` 50 / 50, `BATTLE_FORM_OFF_PCT` 30 / 30 (Top at or above TOP, In at or above IN, Off at or above OFF, else Poor). `validate`: `OFF < IN < TOP` else all three preset. The form reads per-badge config; it is the pet's own reading, not part of the wire contract.
- The spec's form words are renamed for the menu row budget: `fit to fight` → `in form`, `in no state` → `poor form` (owner allows label changes; "Battle: poor form" is 17 columns).

- [ ] **Step 1: write the failing tests** (`mod fight`):
```rust
#[test] fn matrix_matches_section_18_4() {
    // the prototype's `matrix()`: for i, j over builds(), 2000 fights, seed
    // k.wrapping_mul(2654435761).wrapping_add((i * 131 + j * 17) as u32 + 7), percent = wins * 100 / 2000.
    // Assert the 20 "avg" values exactly: [82,84,74,85,86,81,60,41,42,22,9,44,41,48,33,48,44,34,20,2]
    // and the fit-vs-sumo cells (row 0 col 3 = 44, row 3 col 0 = 53). Same arithmetic, same seeds: exact.
    // If a value differs, report the cell and fix derive/fight; never loosen the test.
}
#[test] fn fight_is_deterministic_and_bounded() { /* same seed and snapshots twice -> equal; rounds <= 10; hp pct <= 100 */ }
#[test] fn fight_reads_no_config() {
    // include_str!("../src/game/engine/combat.rs"): the body of `fight` and `win_chance` (text between
    // "pub fn fight" and "pub fn form") contains no "thresholds::" and no "t::"
}
#[test] fn seed_is_symmetric_in_roles() { /* both badges call seed(nonce_a, nonce_b, id_a, id_b) with the challenger first: same value; differs when a nonce changes */ }
#[test] fn chance_is_honest() {
    // win_chance(a, b) == round5(count of fight(chance_seed(k), a, b).a_won for k in 0..64);
    // win_chance(a, a) in 45..=55; fit vs injured >= 95; injured vs fit <= 5; result is a multiple of 5
}
#[test] fn form_words_follow_the_chance() {
    // fit -> Top; hung over -> Poor; injured -> Poor; and for every build b:
    // form(&b) is the band of win_chance(&b, &derive(&Body::REFERENCE)) under the three form keys
}
#[test] fn factors_pick_one_of_each_sign() {
    // sumo two beers (felt 21: tipsy, not sloppy; no hurting factor): [Some(Heavy), Some(Tipsy)]
    // hung over (no helping factor): [Some(HungOver), Some(Thirsty)]
    // well rested with strain 50: [Some(Rested), Some(Sore)]; average: [None, None]
}
#[test] fn decisive_names_the_losers_problem() {
    // wasted (a) vs fit (b), the first seed in 0..100 where b wins: Some((true, Factor(Sloppy)))
    // fat and unfit (a, stamina 1, no hurting factor) vs fit, the first seed where b wins with rounds > 1:
    // Some((true, OutOfSteam(2)))
}
#[test] fn factor_words_fit_half_a_line() { /* every Factor: 1 + text().len() <= 12 */ }
#[test] fn form_words_fit_the_row() { /* every Form: "Battle: ".len() + text().len() <= 17 */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 fight::`.
- [ ] **Step 3: implement** the fight half of `combat.rs` and the form keys; regenerate the template; key list.
- [ ] **Step 4: run them and see them pass.** `... --test battle_v3 fight::`, `... --test engine_v3 template_`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): v3 fight, win chance and form`.
- [ ] **Step 6: V.**

### Task 3: battle rules in the engine: refusals, costs, records, prize, save

Spec 12.2 "What a battle costs", 12.5, 17 row 22 and 18.8 (100 then 20 HAX), and the three guide-stage bugs. After this task the old instant flow still runs, but through the new rules: an injured pet is refused, the target of a heard result only records it and gets the toast, and only a pet that really fought can be paid.

**Files:**
- Modify: `src/game/engine/mod.rs`, `src/game/engine/doses.rs`, `src/game/engine/causes.rs`, `src/game/engine/advance.rs` (`prize_min` counts down), `src/game/engine/thresholds.rs`, `assets/to-badge/BPETS_V3.CFG`, `USER_GAMES.md`, `src/game/lifecycle.rs`, `src/game/modal.rs` (Battle reason), `src/game/battle.rs` (old `challenge` and `on_battle_result` call the new functions), `src/game/battle_view.rs` (reward line shows the HAX actually paid), `src/guide/data.rs` (drop `BATTLE_HAX_REWARD`), `tests/save_format.rs` (dead-slot assertion narrowed to 52, 54 and 56..64).
- Test: `tests/battle_v3.rs` `mod rules`, `mod save`.

**Interfaces:**
- Consumes: Task 1 `snapshot`, Task 2 nothing.
- Produces:
```rust
// mod.rs, GameState (saved in the format-2 head slots that format 3 left at zero)
pub demo_wins: u16,    // offset 38 (old cooldown_feed)
pub demo_losses: u16,  // offset 42 (old cooldown_relax)
pub prize_min: u16,    // offset 44 (old cooldown_play): minutes until a win pays the first prize again
pub decline_min: u16,  // offset 46 (old cooldown_exercise): Task 4 gives it its mood term
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fought { pub won: bool, pub hp_left_pct: u8, pub costs: bool, pub paid: bool }
/// Why this pet cannot start or accept a fight, or None.
pub fn battle_refusal(&self) -> Option<doses::Reason>;
/// The pet fought (`costs`) and/or is recorded. Returns the HAX paid (0 when not paid).
pub fn fight_over(&mut self, f: Fought) -> u32;
/// A fight this pet did not take part in (it heard a fallback RESULT): the record only.
pub fn record_only(&mut self, won: bool);
/// (wins, losses) of the league the pet is in now: demo_* in demo mode, else wins/losses.
pub fn battle_record(&self) -> (u16, u16);
// doses.rs
Refusal::TooWeak   // "too weak"     (health under BATTLE_MIN_HEALTH_PCT)
Refusal::TooDrunk  // "too drunk"    (felt drink over BATTLE_DRUNK_PCT; the spec's "too drunk to fight" is 18 columns)
Refusal::DemoOnly  // "demo pets only" (a demo pet and a player friend; used by Task 7)
Refusal::NoDemo    // "no demo pets"   (a player pet and a demo friend; used by Task 7)
/// Apply the costs of one fight (spec 12.2), the same on both badges.
pub(super) fn battle(s: &mut GameState, won: bool, hp_left_pct: u8);
/// Add `strain` and run the injury rule shared with exercise: over STRAIN_INJ an injury
/// (DOSE_INJURY_MIN, DOSE_INJURY_HEALTH_PCT through demo::floor) and Cause::TornMuscle(why);
/// else, above SORE_PCT, Cause::Strained(why). Returns whether it injured.
fn strain_up(s: &mut GameState, strain: i32, why: StrainWhy) -> bool;
// causes.rs
StrainWhy::Fight   // text "in a fight"; pack the next free StrainWhy code
// lifecycle.rs (wrappers over the singleton, `with_game_now` like friend_boost)
pub fn battle_refusal() -> Option<Reason>;
pub fn fight_over(f: Fought) -> u32;
pub fn record_only(won: bool);
pub fn snapshot() -> Option<combat::Snapshot>;
pub fn battle_form() -> Option<combat::Form>;
```
- `battle_refusal`, in this order: `cannot_act()` (hibernating, egg, gone, busy); asleep → `Refused(Asleep)`; `injured_min > 0` → `Refused(Injured)`; health < `BATTLE_MIN_HEALTH_PCT` → `Refused(TooWeak)`; felt drink (Task 1 `felt` of `Body::of(self)`) > `BATTLE_DRUNK_PCT` → `Refused(TooDrunk)`; `cooldown_battle > 0` → `Wait(cooldown_battle * 10)`. `game_refusal(None)` is removed: mini-games keep `game_refusal(Some(_))`; `PetStats::can_battle` is `battle_refusal().is_none()`; `modal::Item::Battle` takes its reason from `lifecycle::battle_refusal()`.
- `doses::battle` (prototype `battle_cost`): hydration −`BATTLE_WATER_PCT`; strain `lvl(BATTLE_STRAIN_PCT)`, times `BATTLE_STRAIN_NEED_PCT`/100 when hungry, tired (energy < energy lo) or thirsty, times `BATTLE_STRAIN_RESTED_PCT`/100 when rested, through `strain_up(.., StrainWhy::Fight)`; energy −`BATTLE_ENERGY_PCT`; fun +`BATTLE_FUN_PCT`; dirt +`BATTLE_DIRT_PCT`; mood ±`BATTLE_MOOD_PCT`; bruises: health −`(100 − hp_left_pct) * BATTLE_BRUISE_MAX_PCT / 100` percent (in levels: `x * P / 100`), through `demo::floor` when demo is on. `exercise` calls `strain_up` instead of its own injury block (DRY; its behaviour stays identical: `tests/engine_v3.rs` exercise tests stay green).
- `fight_over(f)`: if `f.costs`: `doses::battle(self, f.won, f.hp_left_pct)`, `cooldown_battle = BATTLE_COOLDOWN()`. Record: `wins`/`losses` (+1, saturating), or `demo_wins`/`demo_losses` in demo mode. Prize: if `f.paid && f.won && money_live()`: `BATTLE_PRIZE_FIRST_HAX` when `prize_min == 0`, then `prize_min` = minutes until the next local midnight when `day_min` is `Some(m)` (`1440 − m`), else 1440; otherwise `BATTLE_PRIZE_LATER_HAX`. `add_money(prize)`; return it. `request_save()`.
- `record_only(won)`: the record of the current league, nothing else (no cost, no mood, no cooldown, no HAX). This is the fix for "battle HAX are paid to both sides": a pet that did not fight is never paid.
- `advance`: `prize_min` counts down by `dt` (saturating). Nothing visible reads it, so it is not a solver boundary.
- Save: `to_bytes` writes the four fields at offsets 38, 42, 44, 46 instead of the zeros; `from_bytes` reads them from `Head::cooldowns[0]`, `[2]`, `[3]`, `[4]`; `from_bytes_v2` leaves all four zero (a format-2 save has old cooldowns there). A plan-3 save has zeros there: no battles yet, prize ready.
- Removed (R15): `BATTLE_HAX_REWARD`, `GameState::record_battle`, `lifecycle::record_battle`, its tests (`record_battle_updates_counters_and_cooldown` and the `lifecycle` test at 1489 become `fight_over` tests).
- Transitional, until Task 7: old `battle::challenge` calls `lifecycle::fight_over(Fought { won, hp_left_pct, costs: true, paid: true })` and stores the returned HAX for the result card; `on_battle_result` calls `lifecycle::record_only(we_won)` and `show_toast(if we_won { Toast::BattleWon } else { Toast::BattleLost })` instead of the animation (the target did not fight; this fixes "the toasts are never raised").

**Tunables** (Classic / Casual):

| Key | Classic | Casual | Meaning |
|---|---|---|---|
| `BATTLE_COOLDOWN` (existing, ticks) | 180 | 180 | per-pet rest after a fight: 30 minutes (was 12 ticks) |
| `BATTLE_PAIR_COOLDOWN_MIN` | 60 | 60 | the same two pets (Task 6 reads it) |
| `BATTLE_ENERGY_PCT` | 10 | 10 | cost |
| `BATTLE_WATER_PCT` | 8 | 8 | cost |
| `BATTLE_FUN_PCT` | 20 | 20 | stimulation gained |
| `BATTLE_DIRT_PCT` | 8 | 8 | cost |
| `BATTLE_STRAIN_PCT` | 20 | 20 | strain added |
| `BATTLE_STRAIN_NEED_PCT` | 150 | 150 | times this when hungry, tired or thirsty |
| `BATTLE_STRAIN_RESTED_PCT` | 70 | 70 | times this when well rested |
| `BATTLE_BRUISE_MAX_PCT` | 5 | 2 | health lost at 0 % HP left (health damage: Casual halved, rounded down) |
| `BATTLE_MOOD_PCT` | 8 | 8 | mood + on a win, − on a loss |
| `BATTLE_MIN_HEALTH_PCT` | 30 | 30 | below: too weak |
| `BATTLE_DRUNK_PCT` | 70 | 70 | felt drink above: too drunk |
| `BATTLE_PRIZE_FIRST_HAX` | 100 | 100 | first paid win of the day (17 row 22, 18.8) |
| `BATTLE_PRIZE_LATER_HAX` | 20 | 20 | later wins that day |
| `BATTLE_PRIZE_BEACON_MIN` | 30 | 30 | a fallback fight pays only against a beacon at most this old (Task 7) |
| `BATTLE_DECLINE_MOOD_PCT` | 3 | 3 | Task 4 |
| `BATTLE_DECLINE_MIN` | 60 | 60 | Task 4 |

`validate`: `BATTLE_PRIZE_LATER_HAX <= BATTLE_PRIZE_FIRST_HAX` else both preset; `BATTLE_COOLDOWN` clamped to at least 1.

- [ ] **Step 1: write the failing tests:**
```rust
// mod rules
#[test] fn refusals_follow_the_list() {
    // one Active pet per row, only that row true: hibernating -> Hibernating; asleep -> Refused(Asleep);
    // injured -> Refused(Injured); health 29 % -> Refused(TooWeak); booze 60 % at weight 50 (felt 60)
    // -> None, booze 50 % at weight 0 (felt 75) -> Refused(TooDrunk); a pet 10 minutes past hatching -> None (no age limit);
    // cooldown_battle 6 -> Wait(60); a fit adult pet -> None
}
#[test] fn injured_pet_cannot_battle() { /* stats().can_battle false while injured_min > 0 (guide-stage bug 5) */ }
#[test] fn a_fight_costs_what_the_spec_says() {
    // fit pet, not hungry/tired/thirsty/rested, strain 0, fight_over(won, hp 80, costs, !paid):
    // energy -10 %, hydration -8 %, fun +20 %, dirt +8 %, strain +20 %, mood +8 %, health -1 % ((100-80)*5/100),
    // cooldown_battle == 180, wins + 1
}
#[test] fn a_tired_fighter_strains_more_and_can_tear() {
    // hungry pet: strain +30 %; strain 45 % + 30 % -> injured_min == DOSE_INJURY_MIN, take_causes has
    // TornMuscle(Fight); rested pet: strain +14 %
}
#[test] fn sport_injury_is_unchanged() { /* exercise() with strain 55 % injures exactly as before (same health and cause) */ }
#[test] fn first_win_of_the_day_pays_100_then_20() {
    // money on, day_min Some(20:00): won+paid -> 100, prize_min == 240; again -> 20; advance 240 min -> next pays 100;
    // day_min None: prize_min == 1440; a loss pays 0 and leaves prize_min
}
#[test] fn fallback_target_is_never_paid() {
    // money on, money 50: record_only(true) -> money 50, wins + 1, energy/mood/cooldown unchanged (guide-stage bug 4)
}
#[test] fn unpaid_fight_pays_nothing() { /* fight_over(won, paid: false) -> 0 HAX, costs applied */ }
#[test] fn demo_fights_have_their_own_record() {
    // demo on: fight_over(won) -> demo_wins + 1, wins unchanged, 0 HAX; battle_record() == (demo_wins, demo_losses);
    // demo pet at health 26 %, bruise 5 %: health stays at the demo floor (25 %)
}
#[test] fn battle_reasons_fit() { /* every new Refusal: Reason::Refused(r).text().len() <= 16; pack/unpack round trip */ }
#[test] fn fight_word_fits() { /* StrainWhy::Fight text <= 15 */ }
// mod save
#[test] fn battle_fields_round_trip() { /* demo_wins 0x1234, demo_losses 0x0567, prize_min 777, decline_min 59: from_bytes(to_bytes) equal; bytes[38..40] == 0x1234 LE */ }
#[test] fn plan3_save_loads_with_empty_battle_slots() { /* a to_bytes() with bytes 38..40 and 42..48 zeroed (40..42 is med_cd_min) loads: all four 0 */ }
#[test] fn v2_upgrade_leaves_battle_slots_zero() { /* a copy of save_format's 97-byte GAME_GOLDEN v2 fixture with non-zero old cooldowns at 38..48: from_bytes_v2 -> all four 0 */ }
#[test] fn tail_stays_zero_for_plan_6() { /* every battle field maxed: to_bytes()[160..177] all 0; b[0] == 3; len 177 */ }
```
In `tests/save_format.rs` `v3_golden_bytes_round_trip`, the removed-slot loop becomes `for ofs in [52usize, 54]` and the four battle slots get their own golden values.
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 rules::` and `... save::`.
- [ ] **Step 3: implement**, then regenerate the template and the key list.
- [ ] **Step 4: run them and see them pass.** `... --test battle_v3 rules::`, `... save::`, `... --test save_format`, `... --test engine_v3 template_`, `... --test engine_v3 exercise`, `... --lib -- game::modal game::lifecycle game::battle`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): battle refusals, costs, records and the 100/20 prize`.
- [ ] **Step 6: V.**

### Task 4: the decline penalty (17 row 16)

"Mood target −3 for one hour." A timer that a mood term reads, so it is a solver boundary (R20).

**Files:**
- Modify: `src/game/engine/regime.rs` (`mood_penalty`), `src/game/engine/solver.rs` (`feeds`), `src/game/engine/advance.rs` (`decline_min` counts down), `src/game/engine/mod.rs` (`declined`), `src/game/lifecycle.rs` (`declined`).
- Test: `tests/battle_v3.rs` `mod decline`.

**Interfaces:**
- Consumes: Task 3 `decline_min`, `BATTLE_DECLINE_MOOD_PCT`, `BATTLE_DECLINE_MIN`.
- Produces:
```rust
// mod.rs
pub fn declined(&mut self);  // decline_min = BATTLE_DECLINE_MIN(); request_save()
// lifecycle.rs
pub fn declined();
```
- `mood_penalty` adds `flat(s.decline_min > 0, BATTLE_DECLINE_MOOD_PCT)`. It is a flat term (no truncation), so `TRUNCATING_TERMS` does not change. It is not one of the named `penalty_terms`, so no status word changes.
- `timer_bound`: `s.decline_min` joins the `feeds` list (it ends at `remaining - 1`, like `injured_min`).

- [ ] **Step 1: write the failing tests:**
```rust
#[test] fn decline_lowers_the_mood_target_for_an_hour() {
    // content pet, mood on target: declined() -> regime::mood_target falls by 3 % (in levels: 3 * P);
    // update 60 min -> decline_min == 0 and the target is back
}
#[test] fn decline_penalty_ends_on_time() { /* next_wake_tick from a declined content pet is no later than the minute decline_min reaches 0 */ }
#[test] fn decline_wake_is_exact_on_random_states() {
    // like engine_v3 status_wake::wake_is_exact_on_random_states, 1000 seeds, decline_min random 0..=60:
    // next_wake_tick equals the first minute visible() changes (stepping one minute at a time)
}
#[test] fn decline_is_split_invariant() { /* 200 seeds: one update over 3 h vs the span cut at 30 random ticks: equal to_bytes() */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 decline::`.
- [ ] **Step 3: implement.**
- [ ] **Step 4: run them and see them pass.** `... --test battle_v3 decline::`, `... --test engine_v3 split_invariance::`, `... --test engine_v3 status_wake::`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): declining a fight costs mood for an hour`.
- [ ] **Step 6: V.**

### Task 5: what goes over the radio (spec 12.4, 17 row 21)

**Files:**
- Modify: `src/game/battle.rs` (wire structs only; keep `BattleResultMsg` and its tests as they are), `src/game/engine/combat.rs` (`Snapshot::to_wire`, `from_wire`), `src/game/friends.rs` (`PetBeacon` v3).
- Test: `tests/battle_v3.rs` `mod wire`, `tests/save_format.rs` (v3 beacon golden).

**Interfaces:**
- Consumes: Task 1 `Snapshot`, `Flags`, `WIRE_*`.
- Produces:
```rust
// combat.rs
pub const SNAPSHOT_LEN: usize = 9;
impl Snapshot {
    /// power, guard, speed, hp, stamina, focus (i8 as u8), flags as 3 bytes little-endian.
    pub fn to_wire(&self) -> [u8; SNAPSHOT_LEN];
    /// None when a number is outside its WIRE_* range or flag bits 20, 21 or 22 are set.
    pub fn from_wire(b: &[u8]) -> Option<Snapshot>;
    /// A beacon's six numbers (no condition flags, 17 row 21), clamped into the WIRE_* ranges
    /// (an old badge's numbers are on another scale), with the demo bit.
    pub fn from_beacon(power: u8, guard: u8, speed: u8, hp: u8, stamina: u8, focus: i8, demo: bool) -> Snapshot;
}
// battle.rs
pub const PET_CHALLENGE_TYPE: u16 = 0xBA72;
pub const PET_ACCEPT_TYPE: u16 = 0xBA73;
pub const PET_DECLINE_TYPE: u16 = 0xBA74;
pub struct ChallengeMsg { pub challenger_id: [u8; 2], pub target_id: [u8; 2], pub snap: Snapshot,
                          pub nonce: u16, pub kind: u8, pub name: [u8; PET_NAME_MAX], pub name_len: u8 }   // 29 bytes
pub struct AcceptMsg { pub challenger_id: [u8; 2], pub target_id: [u8; 2], pub snap: Snapshot, pub nonce: u16 } // 15 bytes
pub struct DeclineMsg { pub challenger_id: [u8; 2], pub target_id: [u8; 2], pub why: u8 }                      // 5 bytes
pub const WHY_DECLINED: u8 = 0xFE;  // the player pressed Decline; else `why` is Reason::pack().0
// each: pub fn to_bytes(&self) -> [u8; N]; pub fn from_bytes(buf: &[u8]) -> Option<Self>
// (shorter than N or an invalid snapshot -> None; longer is tolerated, like BattleResultMsg)
// friends.rs
pub struct PetBeacon { /* today's fields, attack..max_hp renamed */ pub power: u8, pub guard: u8, pub speed: u8,
                       pub hp: u8, pub stamina: u8, pub focus: i8, pub demo: bool, pub v3: bool }
const BEACON_SIZE: usize = 25;     // bytes 0..18 as today; 18 power, 19 guard, 20 speed, 21 hp, 22 stamina, 23 focus, 24 flags (bit 0 demo)
const BEACON_SIZE_OLD: usize = 22; // from_bytes: 22..=24 bytes -> stamina 3, focus 0, demo false, v3 false
```
- Byte order is little-endian everywhere, ids first, like `BattleResultMsg`. The old firmware reads the first 22 bytes of a v3 beacon and gets power, guard, speed and HP as its attack, defense, speed and HP (spec 12.4 "Compatibility").

- [ ] **Step 1: write the failing tests:**
```rust
// tests/battle_v3.rs mod wire
#[test] fn snapshot_wire_round_trips_every_field() { /* 2000 random in-range snapshots: from_wire(to_wire) == Some(same), focus -100 and 20 included */ }
#[test] fn out_of_range_snapshot_is_refused() { /* power 71, guard 41, speed 4, hp 121, stamina 11, focus -101, flag bit 21: each -> None */ }
#[test] fn beacon_numbers_are_clamped() { /* from_beacon(100, 100, 100, 150, 3, 0, false): speed 100, hp 120, power 70, guard 40 */ }
#[test] fn packets_round_trip() { /* ChallengeMsg (name "Bram"), AcceptMsg, DeclineMsg: to_bytes len 29 / 15 / 5, from_bytes equal; one byte short -> None */ }
#[test] fn packet_types_are_distinct() { /* the five data_type values (beacon, result, challenge, accept, decline) all differ */ }
// tests/save_format.rs
#[test] fn pet_beacon_v3_golden_bytes() { /* a fixed 25-byte golden; from_bytes(golden) fields; to_bytes == golden */ }
#[test] fn pet_beacon_old_still_decodes() { /* the existing 22-byte BEACON_GOLDEN: stamina 3, focus 0, !demo, !v3 */ }
```
The existing `pet_beacon_golden_bytes_round_trip` becomes `pet_beacon_old_still_decodes` (decode only; a v3 badge never sends 22 bytes).
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 wire::` and `... --test save_format beacon`.
- [ ] **Step 3: implement.** Keep everything compiling with the renamed fields: `on_pet_beacon` stores them (Task 6 extends the record), `friends_view.rs` and the old `battle::challenge` read `power`..`hp`, and the beacon ticker in `src/bin/embassy.rs` fills them from `lifecycle::snapshot()` (Task 3) with `v3: true` and the demo bit (Task 8 moves that into `friends::beacon_now`). Add `src/game/friends_view.rs` and `src/bin/embassy.rs` to the task's files.
- [ ] **Step 4: run them and see them pass.** Same commands, plus `... --lib -- game::battle game::friends`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): battle packets and the v3 beacon`.
- [ ] **Step 6: V.**

### Task 6: friends remember a fighter (spec 12.7 friends.rs)

**Files:**
- Modify: `src/game/friends.rs`.
- Test: `src/game/friends.rs` `mod tests`, `tests/save_format.rs` (friends goldens).

**Interfaces:**
- Consumes: Task 5 `PetBeacon`, `Snapshot::from_beacon`; Task 3 `BATTLE_PAIR_COOLDOWN_MIN`.
- Produces:
```rust
pub struct FriendRecord {
    /* today's fields; attack..max_hp renamed power, guard, speed, hp */
    pub stamina: u8, pub focus: i8,
    pub demo: bool,        // flags bit 0, from the last beacon
    pub v3: bool,          // flags bit 1: the last beacon was a v3 beacon
    pub fought_tick: u32,  // last fight between the two pets (0 = never); rebased on reboot like the other ticks
}
const FRIEND_RECORD_SIZE: usize = 43; // 0..36 as today, 36 stamina, 37 focus, 38 flags, 39..43 fought_tick
pub const FRIENDS_LAYOUT: u8 = 3;     // first byte of `list_v3`
pub const FRIENDS_SAVE_SIZE: usize = 2 + FRIENDS_MAX * FRIEND_RECORD_SIZE; // layout, count, records
pub const FRIENDS_KEY: &str = "list_v3";
pub const FRIENDS_KEY_OLD: &str = "list";  // read for the upgrade, never written
impl FriendRecord {
    pub fn has_numbers(&self) -> bool;               // hp > 0 (a beacon with numbers arrived)
    pub fn estimate_snapshot(&self) -> Snapshot;     // Snapshot::from_beacon(.., self.demo)
    /// Seconds until these two pets may fight again, or None.
    pub fn pair_wait_secs(&self, now_tick: u32) -> Option<u32>;
}
impl FriendsList {
    pub fn from_bytes(buf: &[u8]) -> Self;      // layout 3
    pub fn from_old_bytes(buf: &[u8]) -> Self;  // today's `[count][20 x 36]`; new fields zero
}
pub fn note_fight(device_id: [u8; 2], won: bool, now_tick: u32); // record_battle_vs + fought_tick; dirty
```
- `init`: read `list_v3`; if absent, read `list` with `from_old_bytes` and mark dirty (so the next save writes `list_v3`). `save_if_needed` writes only `list_v3`.
- `on_pet_beacon` caches power to focus, `demo`, `v3` on every beacon, as it does the four numbers today.
- `rebase_after_reboot` includes `fought_tick` (when non-zero).
- `record_battle_vs` stays (the head-to-head record) and `note_fight` calls it.

- [ ] **Step 1: write the failing tests:**
```rust
#[test] fn friends_v3_round_trip() { /* list of 3 with every new field set: from_bytes(to_bytes) equal; len FRIENDS_SAVE_SIZE; byte 0 == 3 */ }
#[test] fn old_friends_blob_migrates() { /* save_format's FRIENDS_PREFIX old blob: from_old_bytes keeps ids, names, records; stamina 0, !v3 */ }
#[test] fn never_writes_the_old_friends_key() { /* include_str!("friends.rs"): the only "set(" call uses FRIENDS_KEY */ }
#[test] fn pair_cooldown_counts_down() { /* fought_tick 1000, now 1000 + 300 ticks (50 min): Some(600); now + 360: None; fought 0: None */ }
#[test] fn rebase_moves_fought_tick() { /* fought_tick is shifted with the others; 0 stays 0 */ }
#[test] fn beacon_updates_the_fighting_numbers() { /* apply a v3 PetBeacon to a record (the same update code on_pet_beacon uses, factored as fn refresh(&mut FriendRecord, &PetBeacon, now)): fields copied, demo and v3 set */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::friends` and `... --test save_format friends`.
- [ ] **Step 3: implement.**
- [ ] **Step 4: run them and see them pass.** Same commands: PASS.
- [ ] **Step 5: commit** `feat(bornpets): friends keep fighting numbers, league and pair cooldown`.
- [ ] **Step 6: V.**

### Task 7: the challenge protocol (spec 12.4 "Flow", 12.5, 17 rows 15 to 17 and 23a)

The heart of the plan: a pure state machine, one per badge, that two tests can wire together. It never touches a static: the caller hands in the pet, the friend record and the clock, and gets back packets and UI effects.

**Files:**
- Create: `src/game/battle_link.rs` (this task only: Task 3's `on_battle_result` and `broadcast_result` moved here unchanged, so `meshcore.rs` keeps compiling; Task 8 rewrites it).
- Modify: `src/game/battle.rs` (add `Session`, `Pending`, `Out`, `PreBattle`, `Note`; remove `CombatStats`, `derive_combat_stats`, `simulate`, `challenge`, `resolve_kind` (dead: a heard result no longer plays the animation) and their tests, R15), `src/game/mod.rs` (`pub mod battle_link`), `src/fw/mesh/meshcore.rs` (the RESULT dispatch points at `battle_link::on_battle_result`), `src/game/lifecycle.rs` (`combat_stats` removed: Task 5 moved the beacon to `snapshot`), `src/game/battle_view.rs` only as far as needed to compile (`try_challenge` shows `Friend not ready` until Task 8 wires `battle_link::challenge`).
- Test: `tests/battle_v3.rs` `mod protocol`.

**Interfaces:**
- Consumes: Tasks 2 (`fight`, `seed`, `win_chance`, `decisive`), 3 (`battle_refusal`, `fight_over`, `record_only`), 4 (`declined`), 5 (messages), 6 (`FriendRecord`).
- Produces:
```rust
// battle.rs
pub const ANSWER_WINDOW_MS: u32 = 20_000;  // challenger waits this long for ACCEPT (spec 12.4)
pub const TARGET_WINDOW_MS: u32 = 15_000;  // the target's buttons close earlier, so its ACCEPT lands inside the 20 s
pub const DEMO_ACCEPT_MS: u32 = 10_000;    // an unattended demo target accepts after this (17 row 15)
pub const LATE_RESULT_MS: u32 = 60_000;    // after an ACCEPT, a RESULT from that peer is ignored this long
/// Who I am, now. `nonce` is fresh entropy for this call (the glue: two platform::entropy_byte()).
pub struct Me<'a> { pub id: [u8; 2], pub kind: u8, pub name: &'a str, pub now_ms: u32, pub now_tick: u32, pub nonce: u16 }
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Role { Challenger, Target }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note { Waiting, Declined, Refused(Reason), NotReady, TimedOut }  // what the pre-battle screen's status line says
#[derive(Clone, Copy, Debug)]
pub struct PreBattle {
    pub role: Role, pub peer_id: [u8; 2], pub peer_kind: u8, pub peer_name: [u8; PET_NAME_MAX], pub peer_name_len: u8,
    pub mine: Snapshot, pub theirs: Snapshot,
    pub chance: u8,          // my point of view
    pub estimate: bool,      // true until the challenger has the real snapshot: shown with "~"
    pub ends_ms: u32, pub note: Note,
}
#[derive(Clone, Copy, Debug)]
pub struct Fight { pub peer_id: [u8; 2], pub peer_kind: u8, pub won: bool, pub my_hp: u8, pub their_hp: u8,
                   pub prize: u32, pub chance: u8, pub why: Option<(bool /*I lost*/, Decisive)>, pub live: bool }
#[derive(Clone, Copy, Debug)]
pub enum Packet { Challenge(ChallengeMsg), Accept(AcceptMsg), Decline(DeclineMsg), Result(BattleResultMsg) }
#[derive(Clone, Copy, Debug)]
pub enum Out {
    Send(Packet),
    Buzz,                                   // the glue passes it through lifecycle::alert_due (never in demo)
    Show,                                   // the pre-battle screen opens or changes (view() has it)
    Close,                                  // it closes
    Fought(Fight),                          // play the animation, fill the result card
    Recorded { peer: [u8; 2], won: bool },  // the glue calls friends::note_fight
    Toast(Toast),                           // BattleWon / BattleLost for a heard result
}
pub type Outs = heapless::Vec<Out, 4>;
pub struct Session { /* pending: Option<Pending>, accepted: Option<([u8; 2], u32)> */ }
impl Session {
    pub const fn new() -> Self;
    pub fn challenge(&mut self, me: &Me, s: &mut GameState, f: &FriendRecord) -> Result<Outs, Reason>;
    pub fn on_challenge(&mut self, me: &Me, s: &mut GameState, m: &ChallengeMsg, pair_wait_secs: Option<u32>) -> Outs;
    /// The target's player: Fight (true) or Decline (false).
    pub fn answer(&mut self, me: &Me, s: &mut GameState, fight: bool) -> Outs;
    pub fn on_accept(&mut self, me: &Me, s: &mut GameState, m: &AcceptMsg) -> Outs;
    pub fn on_decline(&mut self, me: &Me, m: &DeclineMsg) -> Outs;
    /// A 9- or 7-byte RESULT. `sender_demo`: the sender's league from its friend record, None if unknown.
    pub fn on_result(&mut self, me: &Me, s: &mut GameState, m: &BattleResultMsg, sender_demo: Option<bool>) -> Outs;
    /// The window ran out. The challenger needs the friend record for the fallback.
    pub fn on_deadline(&mut self, me: &Me, s: &mut GameState, f: Option<&FriendRecord>) -> Outs;
    pub fn deadline_ms(&self) -> Option<u32>;
    pub fn view(&self) -> Option<PreBattle>;
    pub fn cancel(&mut self) -> Outs;       // demo switched or pet gone: pending dropped, Close
}
```
**Rules** (each is a test below):
- `challenge`: `Err` in this order: a challenge already pending → `Busy`; `f.device_id == me.id` or `!f.has_numbers()` → `Refused(NoNeed)` (a guard: the picker never offers either; it shows `Friend not ready` for a friend without numbers); `s.battle_refusal()`; leagues differ → `Refused(DemoOnly)` (I am demo) or `Refused(NoDemo)`; `f.pair_wait_secs(now_tick)` → `Wait(secs)`. Else: snapshot A now, `Send(Challenge)` with my name and kind, pending Challenger with `theirs = f.estimate_snapshot()`, `estimate = true`, `chance = win_chance(mine, theirs)`, `ends_ms = now + ANSWER_WINDOW_MS`, `Show`.
- `on_challenge`: ignore unless `m.target_id == me.id` and `m.challenger_id != me.id`. Then, first match: pending already → `Decline(Busy)`; hibernating → `Send(Accept)` with `s.snapshot()` at once and nothing else (no screen, no buzz, no cost, no record: 17 row 23a); leagues differ (`m.snap.flags.demo() != s.demo.on`) → `Decline(DemoOnly | NoDemo)`, no buzz, no screen, no cost (12.5); `pair_wait_secs` → `Decline(Wait)`; `battle_refusal` other than `Asleep` and `Wait` → `Decline(that reason)` (an injured, too weak or too drunk target cannot fight; it tells why; no buzz, no screen, no cost); else snapshot B now, pending Target with `chance = 100 - win_chance(m.snap, mine)`, `ends_ms = now + TARGET_WINDOW_MS` (demo: `now + DEMO_ACCEPT_MS`), `Show`, and `Buzz` unless demo. A sleeping target gets the screen too (12.4: woken only if its player presses Fight).
- `answer(true)`: if asleep, `s.wake()` first (the normal grumpy rule). `Send(Accept)` with snapshot B and my nonce; seed = `seed(nonce_a, nonce_b, challenger_id, target_id)`; `o = fight(seed, &a, &b)`; `fight_over(Fought { won: !o.a_won, hp_left_pct: o.b_hp_pct, costs: true, paid: true })`; `Recorded`; `Fought { live: true, .. }`; `accepted = (peer, now)`; `Close`. `answer(false)`: `Send(Decline(WHY_DECLINED))`, `s.declined()`, `Close`.
- `on_accept` (challenger, matching pending): `m.snap` already validated by `from_bytes`; seed as above; `o = fight(seed, &a, &b)`; `fight_over(Fought { won: o.a_won, hp_left_pct: o.a_hp_pct, costs: true, paid: true })`; `Recorded`; `Fought { live: true, chance: win_chance(a, b) }`; pending cleared. An ACCEPT that does not match (wrong peer, no pending, after the window) is ignored.
- `on_decline` (challenger, matching pending): `WHY_DECLINED` → note `Declined`; else note `Refused(Reason::unpack(why, 0))`; the screen stays on that note until any button (Task 9); no record, no cost.
- `on_deadline`, challenger: fallback (17 row 17) only if `f` is `Some` and its league still matches; `o = fight(seed(nonce_a, 0, ..), &a, &f.estimate_snapshot())`; paid only if `now_tick - f.last_seen_tick <= BATTLE_PRIZE_BEACON_MIN * 6`; `fight_over(costs: true, paid)`; `Recorded`; `Send(Result)` in the 9-byte layout; `Fought { live: false }`. Target: demo pet → as `answer(true)`; else note `TimedOut`, `Close` (no cost, no record yet: the RESULT may follow).
- `on_result`: ignore unless I am the target; ignore when `accepted` names that peer within `LATE_RESULT_MS`; ignore when hibernating; ignore when `sender_demo` is `Some(d)` and `d != s.demo.on` (a 7-byte RESULT or an unknown sender counts as a player pet); else `s.record_only(!m.challenger_won)`, `Recorded`, `Toast(BattleWon | BattleLost)`. No HAX, no cost, no animation: the pet did not fight.
- `cancel`: drops a pending challenge (the glue calls it when demo mode is switched or the pet is gone).

- [ ] **Step 1: write the failing tests** (`mod protocol`). A helper builds two adult Active pets `a`, `b` (ids `[1,0]`, `[2,0]`), two `Session`s, friend records of each other from their beacons, and `deliver(out) -> Packet` that pulls the one `Send`:
```rust
#[test] fn live_fight_agrees_on_both_badges() {
    // A.challenge -> Challenge; B.on_challenge -> Show + Buzz; B.answer(true) -> Accept + Fought;
    // A.on_accept -> Fought. a.won == !b.won, a.my_hp == b.their_hp, a.their_hp == b.my_hp, both live
}
#[test] fn chances_add_up_to_100() { /* B's PreBattle.chance + A's chance after ACCEPT (Fought.chance) == 100 */ }
#[test] fn both_pay_costs_in_a_live_fight() { /* energy of both fell by 10 %; both cooldowns 180 */ }
#[test] fn exactly_one_side_is_paid() { /* money on both, 50 each: after the live fight total money == 100 + prize, all of it on the winner */ }
#[test] fn fallback_when_no_answer() {
    // B never answers; A.on_deadline(Some(&friend_b)) -> Send(Result) + Fought{live: false};
    // B.on_result(result) -> Toast(BattleWon|BattleLost) and Recorded; B's energy, mood, money unchanged (bugs 1 and 4)
}
#[test] fn fallback_prize_needs_a_fresh_beacon() { /* friend_b.last_seen_tick 31 min ago: A's Fought.prize == 0 though it won; 29 min: 100 */ }
#[test] fn heard_result_raises_the_toast() { /* a 7-byte RESULT from an unknown challenger naming B: Toast and record_only */ }
#[test] fn decline_costs_the_decliner() { /* B.answer(false): B.decline_min == 60; A.on_decline -> note Declined; no record on either */ }
#[test] fn other_league_is_declined_silently() { /* B demo, A player (stale beacon): B.on_challenge -> only Send(Decline(why = NoDemo)), no Buzz, no Show; A note Refused(NoDemo) */ }
#[test] fn picker_refuses_the_other_league() { /* A demo, friend player: A.challenge == Err(Refused(DemoOnly)) */ }
#[test] fn demo_target_auto_accepts() { /* both demo, B unattended: B.deadline_ms == now + 10 s; B.on_deadline -> Accept + Fought; no Buzz anywhere */ }
#[test] fn hibernating_target_is_untouched() {
    // B hibernating: B.on_challenge -> exactly [Send(Accept)]; b.to_bytes() before == after;
    // A.on_accept -> a normal Fought (17 row 23a)
}
#[test] fn injured_target_declines_with_its_reason() { /* B injured: Decline(why = Injured), no Show; A note Refused(Injured) */ }
#[test] fn sleeping_target_wakes_to_fight() { /* B asleep: Show + Buzz; answer(true): b.is_sleeping == false, fight happened */ }
#[test] fn injured_challenger_is_refused() { /* A injured: A.challenge == Err(Refused(Injured)), nothing sent */ }
#[test] fn target_window_closes_first() { /* B.deadline_ms - now == 15 s < A's 20 s */ }
#[test] fn late_result_after_accept_is_ignored() { /* B accepted; a RESULT from A within 60 s: no Recorded, no Toast, record unchanged */ }
#[test] fn busy_target_declines() { /* B already pending with C: on_challenge(A) -> Decline(Busy) */ }
#[test] fn pair_cooldown_blocks_a_rematch() { /* after a fight, A.challenge(friend_b) == Err(Wait(..)) for 60 min */ }
#[test] fn packets_not_for_me_are_ignored() { /* a CHALLENGE / ACCEPT / RESULT naming another target: no Outs */ }
#[test] fn cancel_drops_the_challenge() { /* pending -> cancel -> view() None, deadline None, Close */ }
#[test] fn session_uses_no_statics() { /* include_str!("../src/game/battle.rs"): no "SyncCell", no "static ", no "lifecycle::", no "fw::" */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 protocol::`.
- [ ] **Step 3: implement** `Session` and remove the old instant battle (R15).
- [ ] **Step 4: run them and see them pass.** `... --test battle_v3 protocol::` and `... --lib -- game::battle`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): challenge, accept and decline between two badges`.
- [ ] **Step 6: V.**

### Task 8: the glue: radio, timer, beacon, buzz

**Files:**
- Modify: `src/game/battle_link.rs` (from Task 7's stub), `src/game/friends.rs` (`beacon_now`), `src/game/lifecycle.rs` (`toggle_demo` calls `battle_link::cancel`), `src/fw/mesh/meshcore.rs` (dispatch), `src/bin/embassy.rs` (beacon ticker, `battle_timer_task` spawned next to the beacon ticker), `src/game/battle_view.rs` (`try_challenge` calls `battle_link::challenge`).
- Test: `src/game/battle_link.rs` `mod tests`.

**Interfaces:**
- Consumes: Task 7 `Session`, `Out`; Task 6 `friends::{get, note_fight}`; Task 3 lifecycle wrappers.
- Produces:
```rust
// battle_link.rs — the one Session of this badge (SyncCell, like friends)
pub fn challenge(friend_index: usize) -> Result<(), Reason>;  // "Friend not ready" stays the view's own check (has_numbers)
pub fn answer(fight: bool);
pub async fn on_challenge(data: &[u8]);
pub async fn on_accept(data: &[u8]);
pub async fn on_decline(data: &[u8]);
pub async fn on_result(data: &[u8]);       // replaces battle::on_battle_result in the dispatch
pub fn on_deadline();                       // called by the firmware timer
pub fn view() -> Option<PreBattle>;
pub fn last_fight() -> Option<Fight>;      // for the result card
pub fn cancel();
/// The deadline the firmware timer must wake for, and a signal that it changed.
pub fn deadline_ms() -> Option<u32>;
pub static DEADLINE_CHANGED: embassy_sync::signal::Signal<CriticalSectionRawMutex, ()>; // embassy-core builds only
```
- Each entry point: catch the pet up (`with_game_now`), build `Me` (`friends::local_device_id`, `lifecycle::pet_kind`, `lifecycle::pet_name`, `platform::uptime_ms_u32`, `lifecycle::now_tick`, nonce from two `platform::entropy_byte()`), call the `Session`, then run the `Out`s: `Send` → `fw::mesh::tx_send` zero-hop on the SHDW slot with the packet's `data_type` (the `broadcast_result` code, generalised; a no-op without the `mesh` feature); `Buzz` → `crate::fw::buzzer::play(crate::PET_WARN_INDEX)` when `alert_due(muted, demo, true)`; `Show` / `Close` → `battle_view::open_pre_battle()` / `battle_view::close()` (Task 9; until then a stub that sets the view active); `Fought` → store for the card and `super::show_battle_anim(..)`; `Recorded` → `friends::note_fight(peer, won, now_tick)`; `Toast` → `show_toast`. After every call, signal `DEADLINE_CHANGED`.
- `battle_timer_task` (firmware): `loop { match deadline_ms() { None => DEADLINE_CHANGED.wait().await, Some(t) => select(Timer::at(instant_of(t)), DEADLINE_CHANGED.wait()).await; if timer won { on_deadline() } } }`. It sleeps until a challenge exists: no polling.
- Beacon ticker: fills the v3 `PetBeacon` from `lifecycle::snapshot()` (power..focus, `demo`), `v3: true`, 25 bytes.
- `meshcore.rs`: the SHDW slot dispatches `PET_CHALLENGE_TYPE`, `PET_ACCEPT_TYPE`, `PET_DECLINE_TYPE` and `PET_BATTLE_TYPE` to `battle_link`.
- The host simulator has no radio: `battle_link` compiles there, `Send` is dropped, and the guide shot (Task 11) drives the `Session` through host-only hooks: `#[cfg(feature = "simulator")] pub fn inject_challenge_for_shot(name: &str, kind: u8)`.

- [ ] **Step 1: write the failing tests:**
```rust
#[test] fn challenge_buzz_follows_the_gate() { /* the Buzz handler calls alert_due: muted -> no play, demo -> no play (a pure fn buzz_due(muted, demo) == alert_due(muted, demo, true)) */ }
#[test] fn toggling_demo_cancels_a_challenge() { /* inject a pending challenge (host hook), lifecycle::toggle_demo(): view() is None */ }
#[test] fn heard_result_updates_the_friend() { /* a friend in the list, on_result(bytes) (block_on): friend's wins/losses moved, fought_tick set */ }
#[test] fn beacon_carries_the_snapshot() { /* a host fn beacon_now() -> PetBeacon (the ticker calls it): numbers == lifecycle::snapshot(), v3, demo bit == is_demo() */ }
```
Factor the ticker's beacon building into `friends::beacon_now() -> Option<PetBeacon>` so it is testable; the ticker only sends it.
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::battle_link`.
- [ ] **Step 3: implement**, then check the firmware compiles: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo check -j 2 --bin embassy --no-default-features --features embassy-1680-full --target thumbv7em-none-eabihf`.
- [ ] **Step 4: run them and see them pass.** `... --lib -- game::battle_link game::friends game::lifecycle` and the `cargo check` above: PASS.
- [ ] **Step 5: commit** `feat(bornpets): battles over the mesh, with a one-shot answer timer`.
- [ ] **Step 6: V.**

### Task 9: the battle screens (spec 12.4 "The pre-battle screen", 12.6)

**Files:**
- Modify: `src/game/battle_view.rs`, `src/game/stat_bar.rs`, `src/game/modal.rs` (Battle row label), `src/game/mod.rs` (route input to the pre-battle screen while it is up; full refresh rules).
- Test: `src/game/battle_view.rs` `mod tests`, `src/game/modal.rs` `mod tests`.

**Interfaces:**
- Consumes: Task 8 `battle_link::{challenge, answer, view, last_fight}`; Task 2 `form`, `factors`, `Form::text`, `Factor::text`, `decisive`; Task 6 friend fields.
- Produces:
```rust
// stat_bar.rs
/// A bar `w` px wide, 6 px high, 1 px outline, filled `pct` from the left, or from the right when `from_right`.
pub fn draw_mirrored_bar<D>(d: &mut D, x: i32, y: i32, w: u32, pct: u8, from_right: bool, color: TriColor) -> Result<(), D::Error>;
// battle_view.rs
pub fn open_pre_battle();   // STATE_PRE (new state), ACTIVE
pub fn picker_row(f: &FriendRecord, mine: &Snapshot, now_tick: u32, i_am_demo: bool) -> heapless::String<24>;
pub fn foot_line(form: Form) -> &'static str;                         // the form word
pub fn factor_line(s: &Snapshot) -> heapless::String<26>;             // "+well rested -hung over"
pub fn decisive_line(why: (bool, Decisive)) -> heapless::String<22>;  // "You were sloppy", "They ran out of steam"
pub fn words() -> std::vec::Vec<String>;                              // host-only, every text below
```
- **Picker** (FONT_7X13 rows as today, 4 per page): `<name> ~<n>% <age>` where name is the friend's name or species, cut to fit, `<n>` = `win_chance(mine, f.estimate_snapshot())`, age = `last_seen` as `<n>m`, `<n>h` or `<n>d` (the diary's formats); a friend of the other league shows `<name> demo` or `<name> player` instead of the estimate and Fire tells why (`demo pets only` / `no demo pets`); a friend without numbers shows only the name. Row ≤ 21 columns (the name is cut first). Title: `Battle <w>W-<l>L`, or `Demo <w>W-<l>L` in demo mode (`battle_record`). Foot, above the hint: the form word (FONT_7X13, centred) and, under it, the pet's own `factor_line` (FONT_6X10, ≤ 25 columns, hurting words in red). Fire on a row runs `battle_link::challenge`; an `Err(r)` shows `r.text()` on the message line (the existing `MSG_*` line now takes any `Reason`).
- **Pre-battle screen** (152 x 152, partial refresh, text and bars only, FONT_6X10 unless noted): y 1 own name left, peer name right-aligned (each ≤ 12 columns); y 14 to 62 five rows, 12 px apart: label `Pow`, `Grd`, `Spd`, `Sta`, `Foc` centred at x 76, own bar x 2 w 60 filled from the left, peer bar x 90 w 60 filled from the right, each `pct = (v - lo) * 100 / (hi - lo)` over its `WIRE_*` range; y 74 `HP <n>` left and `<n>` right; y 86 and 97 each side's two factor lines (`+word` black, `-word` red), ≤ 12 columns each; y 110 the chance in FONT_7X13_BOLD centred: `62% to win`, or `~55% to win` while `estimate`; y 126 the status line: challenger `waiting for <name>` / target `<name> challenges you!` / a `Note` (`declined`, the reason text, `Friend not ready`, `no answer`); y 140 the hint: target `Fire: fight Cancel: no`, challenger nothing while waiting (no cancel while waiting: see Resolved 8), `Any button to close` after a note.
- Input on the pre-battle screen: target Fire/Execute → `answer(true)`; Cancel → `answer(false)`; other buttons nothing. Challenger: nothing while waiting; any button after a note closes it.
- **Result card**: as today, plus: the reward line shows `+<n> HAX` with the amount paid (`Fight::prize`, hidden when 0); a line `chance was <n>%`; the decisive line when `Fight::why` is `Some`. Everything ≤ 21 columns. The cost toasts (`torn muscle` / `strained`, why `in a fight`) are the engine's causes and show on the game screen after the card, like after sport.
- **Play menu row**: `Battle: <form>` when the row is available (form from `lifecycle::battle_form()`), plain `Battle` otherwise (its reason shows when selected, R24); ≤ 17 columns with the wait suffix. Implement in `row_text` for `Item::Battle`; `menu_words` lists `Battle: <form>` and the four forms.
- **Words** (`battle_view::words()`, read by `guide/words.rs` social group): every literal above, templates as `<name>`, `<n>`, `<w>`, `<l>`, the four forms, the 20 factor words with their sign (`+<word>` / `-<word>` as templates plus each word), the decisive templates, `demo`, `player`. Remove words no longer drawn (`Can't battle now`, `On cooldown <n>s`): the message line now shows the `Reason` text, which the guide already lists.

- [ ] **Step 1: write the failing tests:**
```rust
#[test] fn picker_row_marks_the_estimate() { /* friend "Bram" seen 8 min ago: picker_row == "Bram ~<n>% 8m" with <n> == win_chance(..); starts "Bram ~" */ }
#[test] fn picker_row_shows_the_league() { /* demo friend, player me: "Bram demo"; player friend, demo me: "Bram player" */ }
#[test] fn picker_rows_fit() { /* 12-char name, 100 %, 99d: len <= 21 */ }
#[test] fn factor_line_fits() { /* every pair of factor words: factor_line <= 25 columns; empty for an average pet */ }
#[test] fn decisive_lines_fit() { /* every Factor and OutOfSteam(10), both sides: <= 21 */ }
#[test] fn pre_battle_lines_fit() { /* "waiting for " + 12 chars <= 25; 12 chars + " challenges you!" <= 28 -> cut the name to 9 when needed (assert the drawn string <= 25); "Fire: fight Cancel: no" <= 25 */ }
#[test] fn pre_battle_draws_inside_the_panel() { /* draw into test_canvas with a fixed PreBattle: no pixel outside 152 x 152, both 0 % and 100 % bars */ }
#[test] fn result_card_shows_what_was_paid() { /* Fight { prize: 20 }: "+20 HAX" drawn; prize 0: no HAX line */ }
// modal.rs
#[test] fn battle_row_shows_the_form() { /* fit adult pet: row_text(Item::Battle) == "Battle: top form"; injured: "Battle", reason "injured"; every form <= 17 */ }
#[test] fn battle_words_are_listed() { /* tests/guide_words.rs every_drawn_literal_is_listed still passes: run it */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::battle_view game::modal game::stat_bar`.
- [ ] **Step 3: implement.**
- [ ] **Step 4: run them and see them pass.** Same command, plus `... --test guide_words`: PASS.
- [ ] **Step 5: commit** `feat(game): pre-battle screen, picker estimates and form word`.
- [ ] **Step 6: V.**

### Task 10: battles in the simulator (spec 18.4 lifestyles, 18.5 money)

This task builds the measuring tools and writes the long runs. It runs only a 3-day smoke locally; the long runs execute in Task 12.

**Files:**
- Modify: `src/game/engine/sim.rs`, `.github/workflows/tests.yml`.
- Test: `tests/battle_v3.rs` `mod balance`.

**Interfaces:**
- Consumes: Tasks 1 to 4.
- Produces:
```rust
// sim.rs
Act::Battle,   // fight the reference pet now: refused -> counted; else live costs, paid, seed from the run's rng
pub fn fighter(d: u32, m: u32, s: &GameState) -> Plan;      // sporty + Battle at 20:00
pub fn fighter_six(d: u32, m: u32, s: &GameState) -> Plan;  // sporty + Battle at 20:00, 20:30 ... 22:30
pub fn brawler(d: u32, m: u32, s: &GameState) -> Plan;      // party + two beers at 19:30 + Battle at 20:00
pub fn brawler_six(d: u32, m: u32, s: &GameState) -> Plan;  // party + two beers at 19:30 + six battles from 20:00
pub fn sumo(d: u32, m: u32, s: &GameState) -> Plan;         // sporty with rich food while weight < 72 %, + Battle at 20:00
// Outcome gains:
pub fights: u32, pub fights_won: u32, pub fights_refused: u32, pub injuries: u32, pub prizes_hax: u32,
pub win_pct_vs_reference: u8,  // 200 seeded fights per day at 20:00 from that day's snapshot, averaged
```
- `Act::Battle` calls `s.battle_refusal()`; if `None`, `o = combat::fight(seed, &s.snapshot(), &derive(&Body::REFERENCE))` and `s.fight_over(Fought { won: o.a_won, hp_left_pct: o.a_hp_pct, costs: true, paid: true })`. The opponent pays nothing (it is not simulated). Count injuries from `TornMuscle(Fight)` causes.
- CI: add `battle_v3` to the matrix, run as `cargo test --features simulator --test battle_v3 -- --include-ignored --nocapture`, and add it to the `others` skip list.

**Long runs** (`#[ignore]`, Classic, Delta driver unless noted, 30 days, 3 seeds; each `eprintln!`s its numbers for the Task 12 report):
```rust
#[test] #[ignore] fn fit_fighter_one_a_day() { /* fighter: alive; min health >= 70 %; injuries == 0; win_pct_vs_reference >= 70; refused == 0 */ }
#[test] #[ignore] fn fit_fighter_six_a_day_pays_for_it() { /* fighter_six: alive; injuries >= 5; win_pct_vs_reference <= 50; refused >= 20 */ }
#[test] #[ignore] fn brawler_one_a_day() { /* brawler: alive; win_pct_vs_reference <= 45 (18.4: 26) */ }
#[test] #[ignore] fn brawler_six_a_day_is_ruinous() { /* brawler_six: !alive or min health < 30 % */ }
#[test] #[ignore] fn sumo_matches_the_fit_fighter() { /* sumo: alive; |win% - fighter's win%| <= 10 */ }
#[test] #[ignore] fn prize_rule_caps_income() { /* money on, fighter_six: prizes_hax <= 30 * (100 + 5 * 20); fighter: prizes_hax <= 30 * 100 */ }
#[test] #[ignore] fn battle_delta_matches_oracle() { /* fighter and brawler, 10 days, 2 seeds: Delta == Oracle Outcomes */ }
// focused, not ignored:
#[test] fn battle_smoke_three_days() { /* fighter, 3 days, seed 1: alive, fights == 3, injuries == 0, prizes_hax == 0 (money off) */ }
```
- [ ] **Step 1: write the tests** (the smoke fails: `Act::Battle` missing).
- [ ] **Step 2: run and see the smoke fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3 battle_smoke`.
- [ ] **Step 3: implement** `Act::Battle`, the routines, the counters, the CI entry.
- [ ] **Step 4: run the smoke and see it pass.** Same command. Do not run the ignored tests locally.
- [ ] **Step 5: commit** `test(bornpets): battle lifestyles and money in the simulator`.
- [ ] **Step 6: V.**

### Task 11: the guide and the docs

Plan 5 owns guide chapter 11's battle section. Every word Task 9 added is marked and explained, or `make guide` fails.

**Files:**
- Modify: `guide/chapters/11-friends-battles-stations.typ` (the Battles section and the Friends detail card's bars), `guide/chapters/05-care-actions.typ` (576: `won a battle!` and `lost a battle` are raised now; they leave the never-raised list), `guide/chapters/09-money-and-hax.typ` (96: the prize row reads `BATTLE_PRIZE_FIRST_HAX` and `BATTLE_PRIZE_LATER_HAX`), `guide/lib/social.typ` (if `versus()` needs a caption change), `src/guide/shots.rs` (shot `pre-battle`), `src/guide/data.rs` (keys and units checked), `GAME.md` (Battles section), `USER_GAMES.md` (Battles section; the key list was updated by Tasks 1 to 3), `src/game/friends_view.rs` (detail bars read `Pow`, `Grd`, `Spd`, `HP` from the renamed fields; labels ≤ 8 columns).
- Test: `tests/guide_words.rs`, `tests/guide_shots.rs`, `scripts/test_build_guide.py`.

**Content of the Battles section** (plain sentences, numbers only through `#v(...)`, every on-screen word through `#w(...)`): where (the Play row `Battle: <form>` and the four forms, what each means against an average pet); the picker (`~<n>%` is an estimate from the friend's last beacon, with its age; `demo` / `player` rows; the record title, `Demo <w>W-<l>L`; the foot with the form and the pet's own factor words); the challenge (the other badge buzzes, its screen, Fight or Decline, 15 seconds to answer, the challenger waits up to 20; no answer means the fight happens against the last beacon); what the numbers mean (power, guard, speed, stamina, focus, HP, each in one sentence, which care feeds it); the factor words as a table (`+` helps, `-` hurts), all 20; the fight (faster pet first, ten rounds, past its stamina a pet slows; HP left decides after ten rounds); what it costs (energy, water, dirt, strain, bruises, mood, with `#v` values; a tired, hungry or thirsty pet strains more; over the line it tears a muscle, `torn muscle` / `in a fight`); the prize (`#v("BATTLE_PRIZE_FIRST_HAX")` for the first win of the day, then `#v("BATTLE_PRIZE_LATER_HAX")`; only when money is on; a fight against a beacon older than `#v("BATTLE_PRIZE_BEACON_MIN")` pays nothing); declining (`#v("BATTLE_DECLINE_MOOD_PCT")` mood for `#v("BATTLE_DECLINE_MIN")`); who cannot fight and the words it shows (`asleep`, `injured`, `too weak`, `too drunk`, `hibernating`, `wait <time>`, `demo pets only`, `no demo pets`); the other badge's side (a heard result: `won a battle!` / `lost a battle`, record only, no prize, no cost); demo pets fight only demo pets and accept on their own; a frozen pet answers but the fight does not touch it; the result card lines (`chance was <n>%`, the decisive lines). Remove "It cannot hurt the pet", "An injured pet may still fight", "The fight is *instant*" and the old picker table.
- Shot `pre-battle`: start a game, `Poke` the pet adult and fit, `Do(battle_link::inject_challenge_for_shot("Bram", 1))`, expect "the pre-battle screen shows a challenge". Place it beside the challenge paragraph with `#shot("pre-battle")` (the existing macro).

- [ ] **Step 1: run the check and see it fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py`: fails with `on-screen word not marked: ...` for the Task 9 words and `unknown key BATTLE_HAX_REWARD`.
- [ ] **Step 2: write the chapter, the shot and the docs.**
- [ ] **Step 3: run the check and see it pass.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py`: exit 0. Then `... cargo test -j 2 --features simulator --test guide_words --test guide_shots --test guide_data` and `python3 scripts/test_build_guide.py`: PASS.
- [ ] **Step 4: commit** `docs(guide): ch11 battles for BornPets v3`.
- [ ] **Step 5: V.**

### Task 12: regression and integration on CI

One push, the whole suite. Nothing is changed in this task unless a step says so.

**Files:** none (report only).

**Interfaces:**
- Consumes: Tasks 1 to 11.
- Produces: the regression report (in the task report, not a file): every CI job's result, the failing test names with their assert messages, the battle numbers from the long runs (`--nocapture` prints them), the matrix result, and the SSD1680 release image size against 866 KiB before this plan.

- [ ] **Step 1:** run locally, one command at a time: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test battle_v3` (the ignored runs stay off), then `... --test engine_v3 split_invariance::`, `... --test engine_v3 status_wake::`, `... --test save_format`. Fix nothing yet; note failures.
- [ ] **Step 2:** `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh git push github ranzbak/bornpets-v3-battles`.
- [ ] **Step 3:** `gh run list -R ranzbak/bornhack-firmware-2026-ce -b ranzbak/bornpets-v3-battles -L 1 --json databaseId,headSha`. The `headSha` must equal `git rev-parse HEAD`.
- [ ] **Step 4:** `gh run watch <id> -R ranzbak/bornhack-firmware-2026-ce --exit-status`, then `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --json conclusion,jobs`. For each failed job: `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --log-failed`.
- [ ] **Step 5:** check the numbers against spec 18.4 (fit fighter 1/day: health ≥ 84 at its lowest in the prototype, win 83 % against the average pet; 6/day: 16 injuries, 24 %; brawler 1/day 26 %; brawler 6/day gone on day 6; sumo 82 %) and 18.5 (one fight a day ≈ 2600 HAX a month for a fit fighter; three a day 2720 to 3120). The SSD1680 image must be ≤ 960 KiB; report its growth. If over, removing Lights Out goes to Task 13 as its own item.
- [ ] **Step 6:** report: green or the list of failures, the numbers, the image size. If green, Task 13 only records "nothing to fix" and the final numbers.

### Task 13: bug-fix round

Fix what Task 12 found, one item at a time.

**Files:** as each fix needs.

**Interfaces:**
- Consumes: the Task 12 report.
- Produces: fixes; tuned `BATTLE_*` values if any; a final green CI run.

For each failure:
- [ ] **Step 1:** reproduce it with a focused test (an existing one, or a new short one in the matching `tests/battle_v3.rs` module, at most 10 simulated days so it runs locally). See it fail.
- [ ] **Step 2:** fix the cause (superpowers:systematic-debugging: root cause first; a balance failure is tuned only after the engine and protocol are shown correct).
- [ ] **Step 3:** run that focused test and its module. See them pass.
- [ ] **Step 4:** commit `fix(bornpets): <what>` (one commit per item).

Tuning rules (balance failures only):
- Matrix cells differ from 18.4: never tune; find the derive or fight term that differs from the prototype.
- Six fights a day do not hurt enough: raise `BATTLE_STRAIN_PCT` by 2 at a time up to 26, then `BATTLE_BRUISE_MAX_PCT` by 1.
- One fight a day injures a fit fighter: lower `BATTLE_STRAIN_PCT` by 2 down to 16.
- Every tuned value changes both presets where Casual depends on it, regenerates the template (`BPETS_REGEN=1`) and updates the `USER_GAMES.md` key list in the same commit.
- Flash over 960 KiB: remove the Lights Out mini-game in its own commit (owner's allowance), with its menu row, cooldown, docs, guide words and tests.

After the last fix:
- [ ] **Step 5:** push and watch CI as in Task 12 steps 2 to 4. Repeat this task until CI is green. Report the final numbers, the tuned values and the image size.

## On-screen words

| Where | Text |
|---|---|
| Play menu row | `Battle: top form` / `Battle: in form` / `Battle: off form` / `Battle: poor form`; plain `Battle` when refused |
| Reasons (`Refusal::text`) | `too weak`, `too drunk`, `demo pets only`, `no demo pets` (existing: `asleep`, `injured`, `busy`, `hibernating`, `still an egg`, `pet is gone`, `wait <time>`) |
| Picker title | `Battle <w>W-<l>L`, `Demo <w>W-<l>L` |
| Picker rows | `<name> ~<n>% <age>` (age `<n>m`, `<n>h`, `<n>d`), `<name> demo`, `<name> player` |
| Picker foot | the form word; `+<word> -<word>` |
| Factor words (`Factor::text`) | `well rested`, `heavy`, `tipsy`, `cheerful` (+); `injured`, `sloppy`, `hung over`, `too light`, `hungry`, `thirsty`, `craving`, `wired`, `stuffed`, `bored`, `down`, `groggy`, `worn out`, `waterlogged`, `sore`, `filthy` (−) |
| Pre-battle screen | `Pow`, `Grd`, `Spd`, `Sta`, `Foc`, `HP <n>`, `<n>% to win`, `~<n>% to win`, `waiting for <name>`, `<name> challenges you!`, `declined`, `no answer`, `Friend not ready`, `Fire: fight Cancel: no`, `Any button to close` |
| Result card | existing lines; `+<n> HAX` (the amount paid), `chance was <n>%`, `You were <word>`, `They were <word>`, `You ran out of steam`, `They ran out of steam` |
| Cause toast | `torn muscle` / `strained` with why `in a fight` |
| Toasts (now raised) | `won a battle!`, `lost a battle` |

Renamed from the spec, for width (owner allows): `too drunk to fight` → `too drunk` (16-column reason), `fit to fight` → `in form`, `in no state` → `poor form` (17-column row), `in high spirits` → `cheerful` (12-column half line), `Lotte was sloppy` → `They were sloppy` (names do not fit 21 columns), `Fire: fight Cancel: decline` → `Fire: fight Cancel: no`. Removed: `Can't battle now`, `On cooldown <n>s` (the message line shows the `Reason` text instead), `quick` and `parched` (no formula term of their own: `quick` is speed from fitness, `parched` is thirsty).

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| 12.1 weak points: opponent takes no part, uncheckable, stale, free, no why | T7 (live fight both sides), T5 (range check), T6 (estimate with age), T3 (costs), T9 (factors, decisive line) |
| 12.2 / 15.7 five qualities, mass = weight, felt drink, courage tent, floors | T1 |
| 12.2 the fight: 10 rounds, hit%, damage, stamina, coin flip | T2 |
| 12.2 costs, strain multipliers, injury as sport, bruises, mood ±8, cooldowns 30 min / 1 h | T3 (pet), T6 (pair) |
| 12.2 refusals: injured, asleep, health < 30, too drunk, hibernating; baby dropped (owner answer 3) | T3 |
| 12.3 / 18.4 balance matrix and builds | T1 (builds), T2 (matrix, exact), T10 / T12 (lifestyles) |
| 12.4 flow: picker estimate with age, CHALLENGE, buzz, target snapshot, Fight / Decline, 20 s, fallback, decline cost | T7, T8, T9 |
| 12.4 pre-battle screen: mirrored bars, factor lines, exact chance from 64 fights, `~` estimate | T2, T9 |
| 12.4 radio: snapshot 9 bytes, beacon, CHALLENGE / ACCEPT / DECLINE / RESULT, seed, compatibility, cheating limits | T5, T7 |
| 12.5 rewards, records, demo pools, auto-accept, demo record, HAX frozen, spam by costs | T3, T7 |
| 12.6 form word, why lines, decisive line, cost toast, no new art | T2, T9 |
| 12.7 implementation list | T1 to T9 (battle.rs, friends.rs, meshcore, battle_view, stat_bar, engine, lifecycle) |
| 12.8 open questions | answered by 17 rows 16 to 23a; applied below |
| 17 row 16 decline: mood −3 for 1 h | T4 |
| 17 row 17 no answer: the fight happens | T7 (`on_deadline`) |
| 17 row 18 stamina floor 1; row 23 floors 33 / 14 / 60 | T1 |
| 17 rows 19, 20 drink as is, no stance | T1 (no change), not built |
| 17 row 21 flags only inside a challenge; beacon six numbers | T5 (beacon has numbers and the demo bit only) |
| 17 row 22 / 18.8 100 HAX, first win of the day 100, later 20 | T3 |
| 17 row 23a hibernating pet auto-accepts without effect | T7 |
| 17 row 15 unattended demo pet auto-accepts | T7 |
| 17 row 32 Health status dropped: battle record on the Battle screen | T9 (title, foot) |
| 18.5 money; 18.9 "one fight a day pays about two shifts: Accepted" | T3 (rule), T10 / T12 (measured) |
| 8.5 beacon demo flag bit | T5, T6 |
| Guide-stage bugs: toasts never raised; HAX paid to both sides; injured pets can battle | T3 and T7 (`heard_result_raises_the_toast`, `fallback_target_is_never_paid`, `exactly_one_side_is_paid`, `injured_pet_cannot_battle`) |
| Owner: guide ch 11 | T11 |
| Owner: test policy | V step, T12, T13 |

**Resolved conflicts and gaps (planner's calls):**
1. **Save bytes.** The 28-byte reserved tail had 17 left after demo mode, and all 17 are plan 6's. Battles need 8 bytes (demo wins and losses, prize timer, decline timer). They go into four format-2 head slots that format 3 writes as zero and never reads (`cooldown_feed`, `relax`, `play`, `exercise`). Version 3 and 177 bytes stay, a plan-3 save reads as "no battles yet", and the v2 upgrade never maps the old cooldowns there. Free after this plan: head slots 52, 54 (4 bytes) and 56 to 63 (8 bytes), plus plan 6's 17-byte tail.
2. **Friends list.** Its record grows from 36 to 43 bytes. It moves to a new kv key `list_v3` with a layout byte, and the old `list` is read once for the upgrade. Writing the new layout over `list` would break the old firmware's friends, against the spirit of the owner's revert rule.
3. **What is config and what is not.** Snapshot formula magnitudes, costs, refusals, prizes, cooldowns and forms are `BATTLE_*` keys (48 new keys plus `BATTLE_COOLDOWN`). The fight (rounds, hit chance, damage, chance seeds) is const, because both badges must compute the same fight. The formula's slopes and divisors stay literals, as the regime's 3/2 and 1/2 weights do.
4. **Bands move the fight.** The prototype's 78, 83, 38, 33, 73, 58 and 63 are the Classic band edges (18.1 row 3). The port reads the accessors, so Casual and custom bands carry over.
5. **The pre-battle screen is text and bars only.** Spec 12.4 says "text and bars only" and also shows sprites. Five bars, HP, four factor lines, the chance, a status line and a hint fill 152 px; the animation that follows shows both pets.
6. **The exact chance is shown on the result card** ("chance was 62%"), not in an extra refresh before the animation. The animation takes the screen right after the ACCEPT, so the extra refresh would cost a full panel update for a second's view.
7. **Two windows.** The challenger waits 20 s (spec). The target's buttons close at 15 s, so its ACCEPT reaches the challenger inside the 20 s after LoRa air time. Without that, a late ACCEPT would make the target count a live fight that the challenger fought as a fallback. A demo target accepts at 10 s. A RESULT from a peer the target accepted within 60 s is ignored, which covers the remaining race.
8. **No cancel while waiting.** A challenger that walks away after the target pressed Fight would leave the two records out of step. The wait is at most 20 s.
9. **A target that cannot fight** (injured, too weak, too drunk; owner answer 5) declines at once with its reason, without buzzing and at no cost. Showing it a Fight button it cannot use would break R24. A sleeping target is shown the screen, because spec 12.4 lets its player wake it by pressing Fight.
10. **Who is paid.** In a live fight each badge pays its own pet only if that pet won, so exactly one pet is paid. In a fallback fight only the challenger pays costs and can be paid: the full prize when the beacon is at most 30 minutes old (12.4 step 5), nothing when older. The target of a fallback fight, and any target of an old-firmware RESULT, records it, shows `won a battle!` or `lost a battle`, and gets nothing else. This fixes guide-stage bugs 1 and 4.
11. **The per-pet cooldown limits starting a fight, not being challenged.** The challenged player chooses, and the costs limit spam (12.5). The per-pair hour applies on both badges.
12. **Demo is silent.** Spec 12.7 asks for a buzz on an incoming challenge; plan 3's owner answer (e) makes demo mode fully silent. A demo pet gets no buzz. Everyone else's buzz goes through the one gate `alert_due`.
13. **The "power + speed ≤ 130" cheating check is dropped.** It was fitted to the 12.2 formula, which 15.7 replaced. The `WIRE_*` ranges remain, and a property test (T1) proves the formula stays inside them.
14. **The beacon is 25 bytes:** the six numbers (17 row 21, 24 bytes) plus one flag byte for the demo bit that 8.5 and 12.5 require. The old firmware reads the first 22 bytes as before.
15. **Prize "of the day"** is a saved countdown (`prize_min`) to the next local midnight when the clock is set, else 24 hours. Plan 3's `day_min` serves both cases, and nothing visible reads the countdown, so it costs no wake.
16. **The decline penalty is a flat mood term with its own timer.** It needs no status word, and it bounds the segment like `injured_min` (R20).
17. **Factor lines come from flags in a fixed priority order**, not from the size of each term. Both badges see the same flags, so both show the same words (12.4). Sizes would need the other badge's Body.
18. **The `Star` growth-form bit** is reserved (bit 22, always 0) until plan 8 has growth forms.
19. **Old-firmware peers:** a 22-byte beacon becomes a friend with clamped numbers, stamina 3, focus 0 and the player league. The fallback fight works against it. An old badge's RESULT is recorded like a heard fallback.

**Owner answers (2026-11-14):**
1. A challenge with no answer pays the challenger 100 / 20 HAX if the target's last beacon is at most 30 minutes old (`BATTLE_PRIZE_BEACON_MIN`); an older beacon gives a record only. As planned (T3 key, T7 `on_deadline`, `fallback_prize_needs_a_fresh_beacon`, T11 guide).
2. No win-streak counter for now; plan 8 can add it. Nothing built, no save byte.
3. **No age limit for battling.** `BATTLE_BABY_MIN`, `Refusal::TooYoung` and the word `too young` are not in the plan: not in T3's refusal order, key table or tests, not in T7's target refusals, not in T11's guide text or the On-screen words. Spec 12.2's "baby (first 12 h)" refusal is dropped.
4. Cooldowns stay: 30 minutes per pet (`BATTLE_COOLDOWN` 180 ticks), 1 hour per pair (`BATTLE_PAIR_COOLDOWN_MIN` 60). As planned (T3, T6).
5. A target that cannot fight (injured, too weak, too drunk) refuses on its own, without buzzing and without a screen; the challenger sees the reason. As planned (T7 `on_challenge`, `injured_target_declines_with_its_reason`).
Rows 16 to 23a of section 17, 18.8 and 18.9 are applied as written.

**Needs the owner:** nothing open.

**Type consistency:** checked `Body`, `Snapshot`, `Flags`, `Factor`, `Form`, `Outcome`, `Decisive`, `Fought`, `Session`, `Me`, `Pending`, `PreBattle`, `Note`, `Fight`, `Out`, `Packet`, `ChallengeMsg`, `AcceptMsg`, `DeclineMsg`, `BattleResultMsg`, `PetBeacon`, `FriendRecord`, `battle_refusal`, `fight_over`, `record_only`, `declined`, `battle_record`, `snapshot`, `estimate_snapshot`, `pair_wait_secs`, `note_fight`, `win_chance`, `seed`, `chance_seed`, `factors`, `decisive`, `from_wire`, `from_beacon` across the tasks that use them. `Reason` gains no variant (only `Refusal` does), so `Reason::pack` keeps its shape with four new kind codes.

**Review Focus:** items 1 to 5 are pinned in T2 / T5 / T7, T3 / T7, T4, T3 / T6, and T2 / T3 / T7 / T9.

**Proportion:** 11 build tasks: three pure layers (T1 and T2 formula, T5 wire, T7 protocol) carry most of the tests, T3 and T4 the engine, T8 and T9 the glue and screens, T10 the long runs, T11 the guide. The long balance runs are written once (T10) and run once (T12), as the test policy asks.

**Flash:** no font (FONT_6X10 is already linked by the battle animation), no sprite. Added: the snapshot formula and fight (about 2 KB), the protocol state machine and four packet codecs (about 3 KB), the pre-battle screen, picker and card changes (about 2.5 KB), about 0.6 KB of words, 48 new keys (two presets and a setter each, about 2.5 KB), and the friends upgrade (about 0.5 KB). The old `derive_combat_stats`, `simulate` and `challenge` are removed (about 0.8 KB). Estimate: **+10 KiB (8 to 12)**, so about 876 KiB of the 960 KiB gate. Lights Out should not be needed. Task 12 measures it.
