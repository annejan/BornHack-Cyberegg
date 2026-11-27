# BornPets v3, plan 2: reading the pet (UI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** let the player find the optimum by looking. This plan adds a status line under the pet, cause toasts with a diary, a two-page stats view with band ticks, risk markers on menu rows and a "Hint" row where the pet says what it needs. It also removes the Health status entry and moves the idle animation to one frame every 5 minutes.

**Architecture:** the engine (`src/game/engine/`) decides every word. `status_line` names the problem, `conditions` lists the conditions, a transient `Cause` queue records why a rule fired, `preview` dry-runs a dose on a clone, and `hint` maps the status line to a remedy. The UI (`src/game/`) only draws these. Anything on screen that changes by itself is either part of the engine's `Visible` tuple, so `next_wake_tick` stays exact, or it sits in a view that is bounded by the 5-minute UI deadline. Both panels (SSD1675 and SSD1680, both 152 x 152) share one renderer (`draw_graphics` → `game::draw_screen_game`) and one waiter (`wait_display_event` in `src/bin/embassy.rs`). The simulator draws through the same renderer.

**Tech stack:** Rust, `no_std` firmware (embassy, embedded-graphics, FONT_7X13 / FONT_7X13_BOLD only), and host tests through the `simulator` feature.

**Spec:** `BORNPETS_V3_DESIGN.md` (repo root): section 5 (feedback), 17 rows 29 and 32, 18.6, 8.3 (idle pacing), 15.4 and 15.5 (the words for booze, rehab and remedies). Roadmap row 2: `BORNPETS_V3_ROADMAP.md`. Rulings R1 to R26: `.superpowers/sdd/BORNPETS_V3_PLAN_1_ENGINE/global-constraints.md` and the "Ruling:" lines in `progress.md` next to it. Format exemplar: `BORNPETS_V3_PLAN_1_ENGINE.md`.

**Code base:** worktree `BornHack-Cyberegg-v2`, branch `ranzbak/bornpets-v2`, HEAD `7a80ef4`. Anchors at that commit:
- `src/game/engine/status.rs`: `StatusWord` 14, `status` 72.
- `src/game/engine/needs.rs`: fixed lines 12 to 22, `build_edges` 102.
- `src/game/engine/regime.rs`: `sleep_quality_pm` 47, `penalty_terms` 191.
- `src/game/engine/doses.rs`: `Refusal` 15, `Reason` 28, `refusal` 135, `apply` 156, `exercise` 240, `heal` 296, `eat` 397, `sip` 485.
- `src/game/engine/advance.rs`: collapse 185 to 192, `wake` 242.
- `src/game/engine/mod.rs`: `GameState` 224, transient fields 350 to 367, `consume_action_ticks` 638, `banner_bits` 750, `Visible` 895, `refusal_for` 909, `visible` 965, `next_wake_tick` 996, `PetStats` 1053, `stats` 1214.
- `src/game/mod.rs`: `Toast` 56, `status_wants_full_refresh` 387, `next_ui_deadline_tick` 422, simulator sprite blit 564 to 597, toast draw 677, banners 726 to 760.
- `src/game/modal.rs`: Stats items 91, `Item::label` 285, `available` 363, `reason` 444, `activate` 480, `draw_modal` 789, `draw_stats_view` 1047.
- `src/game/input.rs`: modal branch 141.
- `src/game/lifecycle.rs`: `cycle` 387, banner wrappers 403 to 430, `current_severity` 472.
- `src/game/health_view.rs` (whole file); `src/game/battle_view.rs` `draw_picker` 400; `src/lib.rs` view dispatch 637 to 650.
- `src/bin/embassy.rs`: `display_loop` 802, `display_loop_1680` 1080, `wait_display_event` 1439 (sprite branch 1462 to 1511).
- `tests/engine_v3.rs` `mod status_wake` 2147; `.github/workflows/tests.yml`.

## Global Constraints

Every task's requirements include this section.

**Owner rules (verbatim where quoted):**
- "the delta engine as it's written now needs to stay the algorithm to drive progress". No fixed-step loop and no polling. Any new on-screen value that changes over time must either be a wake point through the exact `next_wake_tick` / `Visible` mechanism, or be bounded by the existing UI deadline (`game::next_ui_deadline_tick`).
- Every disabled or refused action shows its reason, from the one mapping (R24: `doses::Reason::text`). A hint never suggests an action the menu would refuse.
- "If the labels in the game need to be changed to make the game flow more clear to the player, do so." Renames are allowed. The e-paper width budgets below still hold.
- Buzz only for danger or for the alerting status words (`StatusWord::alerts`: hungry, thirsty, tired, groggy, hung over, injured, poorly). Nothing in this plan adds a buzz: status-line grades, second words, prompts, cause toasts, menu risks and hints are all silent.
- v3 keeps every balance value tunable as a `Thresholds` field with Classic and Casual values and a `BPETS_V3.CFG` key, and the template is regenerated when one is added. **Plan 2 adds no balance value.** Its new numbers are display constants (`IDLE_FRAME_SECS`, `SECOND_WORD_UNDER`, `RESTLESS_MIN`, `DIARY_LEN`), so the template stays unchanged. See "Needs the owner" in the self-review.
- Never write or delete kv `state`, `game/name` or `BORNPETS.CFG`. Plan 2 persists nothing new: the diary and the cause queue live in RAM only (spec 5.6: "need not be saved"). The save format stays at version 3, 177 bytes, and `to_bytes` is unchanged.
- Local builds use at most 2 cores, and only one cargo command runs at a time: `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo ... -j 2`. The full suite and the firmware builds run on GitHub CI.
- Push only to the `github` remote, never to codeberg.
- Flash budget: the SSD1680 release image is 862 KB of 960 KB. If space runs short, the owner allows removing the Lights Out mini-game. Plan 2 adds no font and no sprite. It removes `health_view.rs` and its icons. Each task reports the release image size printed by CI.
- Commit footer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**Carried from plan 1:** integer math only in `src/game/engine/` (except `sim.rs`); no `alloc` in engine code; levels are `u16` with `P = 655` per percent; 1 engine minute = 6 ticks; engine wake cap 720 ticks; R15 (code this plan makes dead is removed in the same commit, with no commented-out code); R24; R26 (call it "BornPets v3"); R7 and R13 (the known pre-existing failures `name_screen::tests::drops_control_and_beyond_latin1` and the `signed_channel.rs` doctest are not yours).

**Width budgets** (FONT_7X13 and FONT_7X13_BOLD are 7 px per column; both panels are 152 x 152):

| Where | Budget |
|---|---|
| Toast line (top-left at x = 2; HAX is right-aligned at x = 150) | ≤ 15 columns, at most 2 lines |
| Status line (bottom of the pet area, centred) | ≤ 21 columns per line, at most 2 lines |
| Popover body and title (modal inner 128 px) | ≤ 18 columns |
| Menu row (label at inner x + 4) | ≤ 17 columns, price and marker included, in normal and hard mode |
| Full-screen view line | ≤ 21 columns |
| Stats bar label column | ≤ 8 columns |

Each text table gets a worst-case width test in the task that adds it.

**Verify (V)**, the last step of every task:
1. Run the task's focused tests locally (the command is given in the task).
2. `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh git push github ranzbak/bornpets-v2`.
3. `gh run list -R ranzbak/bornhack-firmware-2026-ce -b ranzbak/bornpets-v2 -L 1 --json databaseId,headSha`. The `headSha` must equal `git rev-parse HEAD`.
4. `gh run watch <id> -R ranzbak/bornhack-firmware-2026-ce --exit-status`.
5. `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --json conclusion`. Expected: `{"conclusion":"success"}`.

From Task 1 on, CI builds both panels (`embassy-game` on SSD1675, `cargo fw-1680` on SSD1680) and gates the SSD1680 release image at 960 KiB. Copy the printed image size into the task report.

## Review Focus

These are the failure modes most likely to reach a player unnoticed, most likely first. Each one is pinned by a test in the task named.

1. **The status line goes stale.** A grade (peckish → hungry), a second word, a prompt or an info word (tipsy, sore, well rested) changes between engine wakes. The line then shows yesterday's word for up to 2 h. Pinned in Task 1 by `wake_is_exact_on_random_states`, which seeds 2000 states and requires `next_wake_tick` to equal the first minute the `Visible` tuple changes. It also covers a top-2 order flip caused by integer rounding.
2. **The 5-minute idle frame hides a wake.** The waiter sleeps a full idle frame while a status change, an action end or a toast deadline falls inside it. Or a wake is reported as "advance the frame". Pinned in Task 3 by `sprite_wait_ends_at_the_engine_wake` and `idle_frame_is_five_minutes_actions_stay_fast`.
3. **The hint and the menu disagree.** For example, "play with me" while Play is refused for an injury, or "feed me" with 0 HAX in money mode. Pinned in Task 9 by `hint_and_menu_agree` and `suggested_action_is_never_refused`.
4. **`preview` changes the pet, or predicts something the dose does not do.** Examples: a preview that drains the cause queue, charges money, or wakes the real pet; a risk marker for a food coma that does not happen. Pinned in Task 4 by `preview_never_changes_the_pet` and `preview_matches_what_the_dose_does`.
5. **Text runs off the panel in the worst case.** Hard-mode prices with a risk marker, a 4-digit battle record, a 12-character pet name in "<name> says", "but wait 72h00", and two 11-letter status words. Pinned by the width tests in Tasks 1, 5, 7, 8 and 9.

## File map

| File | Change |
|---|---|
| `src/game/engine/status.rs` | `Word`, `StatusLine`, `status_line`, `danger_word`, `Condition`, `Conditions`, `conditions` |
| `src/game/engine/needs.rs` | named fixed lines `HUNGRY_PCT`, `EXHAUSTED_PCT`, `TIPSY_PCT`, `SORE_PCT` used by `build_edges` |
| `src/game/engine/causes.rs` | create: `Cause`, `StrainWhy`, `Unsettled`, `CauseQueue` |
| `src/game/engine/hint.rs` | create: `Remedy`, `Subject`, `Ask`, `Hint`, `hint` |
| `src/game/engine/regime.rs` | `unsettled_by`; `sleep_quality_pm` uses it |
| `src/game/engine/doses.rs`, `advance.rs` | emit causes; `price` |
| `src/game/engine/mod.rs` | `Visible` uses `StatusLine`; transient `causes`, `bed_why`; `take_causes`, `preview*`, `remedy_reason`, `onset_bits`; `PetStats` `line`, `ceiling`, `conditions` |
| `src/game/pacing.rs` | create: idle and action frame pacing |
| `src/game/diary.rs`, `src/game/diary_view.rs` | create |
| `src/game/stats_pages.rs` | create: the two pages as tables |
| `src/game/test_canvas.rs` | create (`#[cfg(test)]`): 152 x 152 `DrawTarget` for render tests |
| `src/game/mod.rs` | status line drawing, banners removed, `Toast::Cause`, `live_view_open`, UI deadline |
| `src/game/modal.rs` | stats pages and flip, Diary / Hint rows, hint view, risk markers and title |
| `src/game/stat_bar.rs` | band ticks |
| `src/game/lifecycle.rs` | `status_line`, cause drain into the diary, preview wrappers; banner wrappers removed |
| `src/game/input.rs`, `src/lib.rs` | Left / Right paging, diary view; Health status removed |
| `src/game/health_view.rs` | delete |
| `src/game/battle_view.rs` | own record in the picker title |
| `src/bin/embassy.rs` | `wait_display_event` uses `pacing` (both panel loops call it) |
| `.github/workflows/tests.yml`, `scripts/flash_size.py` | SSD1680 build and flash gate |
| `GAME.md`, `USER_GAMES.md`, `README.md`, `FAQ.md` | screen descriptions, each in the task that changes the screen |
| `tests/engine_v3.rs`, `tests/engine_v3_casual.rs` | engine tests |

---

### Task 1: status line model, conditions, wake exactness, CI on both panels

**Files:**
- Modify: `src/game/engine/status.rs`, `src/game/engine/needs.rs` (fixed lines, `build_edges` 102), `src/game/engine/mod.rs` (`Visible` 895, `visible` 965).
- Modify: `.github/workflows/tests.yml` (firmware job). Create: `scripts/flash_size.py`.
- Test: `tests/engine_v3.rs` (new `mod status_line`; update `mod status_wake` 2147).

**Interfaces:**
- Consumes: `regime::penalty_terms`, `status::status`, `GameState::{is_diabetic_unmedicated, is_alcoholic_untreated, banner_bits}`, `needs::*`.
- Produces:
```rust
// needs.rs: fixed edges that already exist, now named (one const, used by build_edges, status and conditions)
pub const HUNGRY_PCT: u8 = 25;     // fullness fixed edge
pub const EXHAUSTED_PCT: u8 = 10;  // energy fixed edge
pub const TIPSY_PCT: u8 = 10;      // booze fixed edge
pub const SORE_PCT: u8 = 40;       // strain fixed edge
// status.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Word { Peckish, Hungry, Starving, Stuffed, Thirsty, Parched, Waterlogged, Sleepy, Exhausted, Bored, Wired,
    Tipsy, Sloppy, Wasted, Grubby, Filthy, Overweight, Underweight, Unfit, HungOver, Sore, Injured, Groggy, Poorly,
    NoInsulin, NeedsRehab, Asleep, WellRested, Content }
impl Word { pub fn text(self) -> &'static str; pub const ALL: [Word; 29]; }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatusLine { pub first: Word, pub second: Option<Word>, pub danger: bool }
impl StatusLine { pub fn text(&self) -> heapless::String<32>; }   // "a, b" when ≤ 21 cols, else "a\nb"
pub const SECOND_WORD_UNDER: i32 = 10 * P;
pub fn danger_word(s: &GameState) -> Option<Word>;
pub fn status_line(s: &GameState) -> StatusLine;
#[derive(Clone, Copy, PartialEq, Eq, Debug)] #[repr(u8)]
pub enum Condition { Injured, Sore, HungOver, Groggy, WornOut, Diabetic, Dependent, InRehab, WellRested }
impl Condition { pub fn text(self) -> &'static str; pub const ALL: [Condition; 9]; }
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Conditions(pub u16);
impl Conditions { pub fn has(self, c: Condition) -> bool; pub fn iter(self) -> impl Iterator<Item = Condition>;
    pub fn line(self, cols: usize) -> heapless::String<24>; }
pub fn conditions(s: &GameState) -> Conditions;
// mod.rs
type Visible = (status::StatusLine, DisplayAnim, bool, Phase, Option<Action>, u8);   // StatusLine carries danger
```

**Rules (the decisions the tests pin):**

Graded words, from the `StatusWord` of a penalty term:
- Hungry → `Starving` below `NEED_DANGER_LO_PCT`, `Hungry` below `HUNGRY_PCT`, else `Peckish`.
- Thirsty → `Parched` below `NEED_DANGER_LO_PCT`, else `Thirsty`.
- Tired → `Exhausted` below `EXHAUSTED_PCT`, else `Sleepy`.
- Sloppy → `Wasted` above `booze_danger()`, else `Sloppy`.
- Grubby → `Filthy` above `DIRT_DANGER_PCT`, else `Grubby`.
- Every other `StatusWord` maps one to one.

`danger_word` checks, in this order: fullness → `Starving`, hydration → `Parched`, booze → `Wasted`, dirt → `Filthy`, health below `HEALTH_DANGER_PCT` → `Poorly`. `status()` keeps its signature and computes `danger` as `danger_word(s).is_some()`, so the rule exists once.

`status_line` builds its candidate words in this order, deduplicated:
1. `danger_word`;
2. the graded word of the largest penalty term above `CONTENT_UNDER` (a tie goes to the earlier term, as today);
3. the prompt: `NoInsulin` if `is_diabetic_unmedicated`, else `NeedsRehab` if `is_alcoholic_untreated`;
4. the graded word of the second-largest term above `SECOND_WORD_UNDER`.

`first` and `second` are the first two candidates. With no candidate, `first` is the first info word that holds: `Asleep` (`is_sleeping`), `Sore` (`conditions` has Sore), `Tipsy` (booze ≥ `TIPSY_PCT`), `WellRested` (`rested_min > 0`), else `Content`. `danger` is `status().1`.

`conditions`, one bit per `Condition`:
- Injured: `injured_min > 0`.
- Sore: strain > `SORE_PCT` and not injured.
- HungOver: `hangover_min > 0`.
- Groggy: `groggy_min > 0`.
- WornOut: `deprived`.
- Diabetic: `diabetic`.
- Dependent: `dependent && rehab_min == 0`.
- InRehab: `rehab_min > 0`.
- WellRested: `rested_min > 0`.

`line(cols)` takes the conditions in enum order (most pressing first) and returns the largest k ≤ 2 for which `join(first k, ", ")`, plus `" +{n-k}"` when n > k, fits `cols`.

Every line these words read is already a threshold edge, a `solver::sig` line or a timer bound, so `Visible` stays exact without solver changes. If one is not (the implementer checks each), make it one in `build_edges` or `sig` and say so in the report.

- [ ] **Step 1: write the failing tests** in `tests/engine_v3.rs` `mod status_line` (Classic preset; `pet()` as in `status_wake`):
```rust
#[test] fn graded_words_follow_the_fixed_edges() {
    // (field, pct) -> expected first word, danger
    // fullness 30 -> Peckish,false; 20 -> Hungry,false; 14 -> Starving,true
    // hydration 30 -> Thirsty; 14 -> Parched,true
    // energy 30 awake -> Sleepy; 9 -> Exhausted
    // dirt 60 -> Grubby; 86 -> Filthy,true
    // booze 45 -> Sloppy; 71 -> Wasted,true
}
#[test] fn two_large_terms_show_two_words() { /* fullness 20 (27 P), fun 10 awake (23 P): (Hungry, Some(Bored)) */ }
#[test] fn a_small_second_term_is_dropped() { /* fullness 20, fun 30 (3 P): second == None */ }
#[test] fn the_danger_word_comes_first() { /* dirt 86, fun 0 awake: (Filthy, Some(Bored)), danger */ }
#[test] fn prompts_take_the_second_slot() { /* diabetic, insulin_min 0, fullness 20: (Hungry, Some(NoInsulin));
                                               dependent, rehab 0, all in band: (NeedsRehab, None) */ }
#[test] fn info_words_when_nothing_is_out() { /* asleep -> Asleep; strain 45 -> Sore; booze 12 -> Tipsy;
                                                rested_min 30 -> WellRested; fresh -> Content */ }
#[test] fn status_text_fits_two_lines_of_21() { /* every (a, Some(b)) and (a, None) over Word::ALL:
                                                   each '\n'-line <= 21 chars, at most 2 lines;
                                                   (Hungry, Some(Bored)).text() == "hungry, bored" */ }
#[test] fn condition_line_names_the_two_most_pressing() {
    // {Injured, HungOver}.line(18) == "injured, hung over"
    // {Injured, HungOver, Diabetic}.line(18) == "injured +2"
    // empty -> ""; every Condition::text() <= 11 chars
}
```
In `mod status_wake`:
- change `obs` to `(status_line(s), s.display_anim(), s.is_sleeping, s.phase, s.active_action, s.banner_bits())`;
- add these `cases()`: "peckish turns hungry" (fullness 26), "second word appears" (fullness 40, fun 20 awake), "tipsy fades" (booze 11, else in band), "sore fades" (strain 42), "well rested ends" (`rested_min = 30`), "prompt appears" (diabetic, `insulin_min = 20`);
- add `wake_is_exact_on_random_states`: an LCG with seed 1 and 2000 states, each need uniform in 0..=100 %, `is_sleeping` random, and the timers `rested_min`, `hangover_min`, `injured_min`, `insulin_min` random in 0..=200. For each state, `next_wake_tick() == first_change_min(s).map_or(t0 + 720, |m| t0 + m * 6)`. On failure, print the seed index.

- [ ] **Step 2: run them and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo test -j 2 --features simulator --test engine_v3 status_` fails to compile (`status_line` missing).

- [ ] **Step 3: implement** the constants, `Word`, `StatusLine`, `danger_word`, `status_line`, `Condition`, `Conditions`, `conditions` and the `Visible` change, following the rules above. `next_wake_tick`'s doc comment names `StatusLine` as the view.

- [ ] **Step 4: add CI coverage for both panels.** Add these steps to the `firmware` job in `.github/workflows/tests.yml`, after the existing build:
```yaml
      - run: cargo build --bin embassy --no-default-features --features embassy-1680-full --target thumbv7em-none-eabihf
      - run: rustup component add llvm-tools
      - run: cargo build --release --bin embassy --no-default-features --features embassy-1680-full --target thumbv7em-none-eabihf
      - run: python3 scripts/flash_size.py target/thumbv7em-none-eabihf/release/embassy --limit-kib 960
```
`scripts/flash_size.py` does three things:
- it runs `llvm-objcopy -O binary` from `$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/`, which is the image `Makefile:147` produces;
- it prints `flash image: <bytes> B (<KiB> KiB of <limit> KiB)`;
- it exits 1 above the limit.

Python standard library only.

- [ ] **Step 5: run them and see them pass.** `... --test engine_v3 status_` and `... --test engine_v3 wake_`: PASS. Run `python3 scripts/flash_size.py --help`: exits 0.

- [ ] **Step 6: commit.**
```bash
git add src/game/engine/status.rs src/game/engine/needs.rs src/game/engine/mod.rs tests/engine_v3.rs .github/workflows/tests.yml scripts/flash_size.py
git commit -m "feat(engine): status line words and conditions as exact wake points; CI builds both panels"
```
- [ ] **Step 7: V.** The CI log shows both firmware builds and the flash line.

### Task 2: status line on the game screen; corner banners removed

Spec 5.2: "One status line under the pet (replaces the corner banners)". The pet sprite fills the pet area (152 x 75 at y = 35), so the line is drawn at the bottom of that area on a white backing. It is bottom-centred at (76, `SEP_BOT` - 2). Each text line gets a backing of its own width + 4 px. The text is bold black, or bold red when `danger` (spec: "red only for health danger zones"). The line is drawn only in the "Active game" branch and not while hibernating.

**Files:**
- Modify: `src/game/mod.rs` (banners 726 to 760 out, draw call after the toast, `status_wants_full_refresh` 387), `src/game/lifecycle.rs` (banner wrappers 403 to 430 out), `src/game/engine/mod.rs` (`banner_bits` → `onset_bits`).
- Create: `src/game/test_canvas.rs`.
- Modify: `GAME.md` ("The Game Screen"), `USER_GAMES.md` (BornPets "Stats").
- Test: `src/game/mod.rs` tests, `tests/engine_v3.rs` `status_wake`.

**Interfaces:**
- Consumes: Task 1 `status_line`, `StatusLine::text`.
- Produces:
```rust
// lifecycle.rs
pub fn status_line() -> Option<engine::status::StatusLine>;   // cheap read, no update (like display_anim)
// mod.rs
pub fn draw_status_line<D: DrawTarget<Color = TriColor>>(display: &mut D, line: &StatusLine) -> Result<(), D::Error>;
// engine mod.rs: replaces banner_bits in Visible; bit 0 diabetic, bit 1 dependent (the onset takeovers read them)
pub fn onset_bits(&self) -> u8;
// test_canvas.rs (#[cfg(test)])
pub struct Canvas { /* [TriColor; 152 * 152], WHITE-filled */ }
impl Canvas { pub fn new() -> Self; pub fn get(&self, x: i32, y: i32) -> TriColor; pub fn any(&self, area: Rectangle, c: TriColor) -> bool; }
impl DrawTarget for Canvas { type Color = TriColor; type Error = core::convert::Infallible; /* out-of-bounds pixels ignored */ }
```
- Removed (R15): `lifecycle::{is_diabetic_unmedicated, is_alcoholic_untreated, is_overweight_warning, is_drunk_warning}`; `GameState::{is_overweight_warning, is_drunk_warning, banner_bits}` if no reader is left. `GameState::is_diabetic_unmedicated` and `is_alcoholic_untreated` stay (the prompts read them).
- `status_wants_full_refresh` drops the four banner terms and adds `lifecycle::status_line().is_some_and(|l| l.danger)`. Red seats only on the SSD1675 full path. The SSD1680 loop drives every update fast, as today.

- [ ] **Step 1: write the failing tests.**
```rust
// src/game/mod.rs tests
#[test] fn status_line_draws_at_the_pets_feet() {
    // Canvas; draw_status_line(&StatusLine { first: Word::Hungry, second: None, danger: false })
    // black pixels exist in y 96..=110; nothing drawn above y 84; no RED pixel
}
#[test] fn danger_line_is_red() { /* first: Starving, danger: true -> RED pixels in y 96..=110 */ }
#[test] fn two_long_words_take_two_lines_inside_the_pet_area() {
    // (Waterlogged, Some(Underweight)): pixels in both 13-px rows, all within y 82..=110 and x 0..152
}
#[test] fn full_refresh_follows_danger_not_banners() {
    // lock_test_state; active pet, weight 75 % (old OVERWEIGHT banner): false; fullness 10 %: true
}
```
- In `tests/engine_v3.rs`: rename `banner_flips_are_wake_points` to `onset_flips_are_wake_points` (the diabetes and dependence onset cases). `obs` uses `onset_bits`.

- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo test -j 2 --features simulator --lib game::tests` fails (`draw_status_line` missing).

- [ ] **Step 3: implement.** Add the drawing, the lifecycle accessor and `onset_bits`. Remove the banners, their wrappers and `banner_bits`. Fix the comments that point at banners: `engine::to_display` ~127 and the 11-column note in `debug.rs` (the toast budget is now 15 columns; see the width table).

- [ ] **Step 4: update the docs.** `GAME.md` "The Game Screen": add the status line to the diagram, with its vocabulary (the "Status line" group in "On-screen words") and the red rule. The NEEDS MEDS, NEEDS REHAB, OVERWEIGHT and DRUNK banners are gone; mention them as `no insulin`, `needs rehab`, `overweight`, `sloppy`. `USER_GAMES.md` "Stats" gets one paragraph on the status line.

- [ ] **Step 5: run them and see them pass.** `... --lib game::tests` and `... --test engine_v3 wake_`: PASS. Run `cargo sim` once and look at a hungry pet: the line is legible over the sprite.

- [ ] **Step 6: commit** `feat(game): status line under the pet replaces the corner banners`.
- [ ] **Step 7: V.**

### Task 3: the 5-minute idle frame; live views bounded by the UI deadline

Spec 17 row 29: "One idle frame every 5 minutes in normal play too; action animations still play at full speed". Today every multi-frame non-action animation advances every 3 s (`embassy.rs` 1499).

**Files:**
- Create: `src/game/pacing.rs`.
- Modify: `src/bin/embassy.rs` (`wait_display_event` sprite branch 1462 to 1511, plus the two select arms that return `true`), `src/game/mod.rs` (simulator sprite blit 564 to 597, `next_ui_deadline_tick` 422), `src/game/modal.rs`.
- Modify: `FAQ.md` (lines 11 to 13 and the 33 table row), `GAME.md` "The Game Screen".
- Test: `src/game/pacing.rs` tests, `src/game/mod.rs` tests.

**Interfaces:**
- Produces:
```rust
// pacing.rs
pub const IDLE_FRAME_SECS: u64 = 300;
pub const IDLE_FRAME_TICKS: u32 = 30;
pub const MIN_FRAME_SECS: u64 = 2;
/// Hatching and the action animations (Feeding, Healing, Playing, Exercising, Medicating, Drinking, Ozempic,
/// Rehab, OnlyPets) are "fast"; everything else (Idle, Happy, Warning*, Critical*, Sleeping, Leaving, Gone,
/// Hibernating) is idle.
pub fn is_action_anim(anim: DisplayAnim) -> bool;
/// Seconds per frame, None for a single-frame animation. Hatching: remaining ticks * 10 / frames; action
/// animations: action_ticks_remaining * 10 / frames; both floored at MIN_FRAME_SECS. Idle: IDLE_FRAME_SECS.
pub fn frame_interval_secs(anim: DisplayAnim, frames: u32, action_ticks_remaining: u8) -> Option<u64>;
/// How long to wait and whether the wait ends on a frame (true) or on a wake (false, redraw without advancing).
pub fn sprite_wait(frame_secs: Option<u64>, wake_secs: u64) -> (u64, bool);
// mod.rs
pub fn live_view_open() -> bool;   // views whose numbers change by themselves; Task 3: the stats view
// modal.rs
pub fn stats_view_open() -> bool;
```
- `sprite_wait`: with `Some(f)` and `f <= wake_secs`, return `(f, true)`; otherwise `(wake_secs, false)`.
- `wait_display_event` computes `wake_secs = (next_wake_tick().min(next_ui_deadline_tick()) - now).max(1) * 10` on both the single-frame and the multi-frame paths. The `sprite_tick` future yields the bool from `sprite_wait`, and the two select arms return that bool instead of `true`. Both panel loops call this function, so both get the change.
- The simulator blit computes `frame = elapsed_s / interval` with `interval = frame_interval_secs(..).unwrap_or(u64::MAX)`, so the simulator shows the same pace.
- `next_ui_deadline_tick` returns `min(existing, now + IDLE_FRAME_TICKS)` while `live_view_open()`. This is the bound the owner rule allows for views whose numbers drift (percentages, ages). It refreshes at most once per idle frame, which is the same budget as the idle animation.

- [ ] **Step 1: write the failing tests.**
```rust
#[test] fn idle_frame_is_five_minutes_actions_stay_fast() {
    assert_eq!(frame_interval_secs(DisplayAnim::Idle, 3, 0), Some(300));
    assert_eq!(frame_interval_secs(DisplayAnim::Sleeping, 4, 0), Some(300));
    assert_eq!(frame_interval_secs(DisplayAnim::WarningHungry, 2, 0), Some(300));
    assert_eq!(frame_interval_secs(DisplayAnim::Exercising, 3, 6), Some(20));   // 6 ticks * 10 / 3
    assert_eq!(frame_interval_secs(DisplayAnim::Feeding, 5, 1), Some(2));       // floor
    assert_eq!(frame_interval_secs(DisplayAnim::Hatching { ticks_remaining: 12 }, 4, 0), Some(30));
    assert_eq!(frame_interval_secs(DisplayAnim::Idle, 1, 0), None);
}
#[test] fn sprite_wait_ends_at_the_engine_wake() {
    assert_eq!(sprite_wait(Some(300), 120), (120, false));
    assert_eq!(sprite_wait(Some(300), 300), (300, true));
    assert_eq!(sprite_wait(Some(300), 7200), (300, true));
    assert_eq!(sprite_wait(None, 50), (50, false));
}
// src/game/mod.rs tests
#[test] fn an_open_stats_view_refreshes_within_an_idle_frame() {
    // lock_test_state; active pet; open Stats, activate ViewStats: next_ui_deadline_tick() <= now + 30;
    // close the modal: next_ui_deadline_tick() == u32::MAX (no toast, no sleep floor)
}
```
- [ ] **Step 2: run and see them fail.** `... --lib pacing` and `... --lib game::tests::an_open`.
- [ ] **Step 3: implement** `pacing.rs`, the waiter change, the simulator blit, `live_view_open` and the deadline.
- [ ] **Step 4: update the docs.** `FAQ.md`: BornPet no longer wakes "every few seconds". The idle frame advances once per 5 minutes; actions animate at full speed. Change the battery table row to match (no longer "Highest"; say "one frame per 5 min, plus events"). `GAME.md`: one sentence on the calm idle.
- [ ] **Step 5: run them and see them pass.** Firmware compile is checked by CI (both panels).
- [ ] **Step 6: commit** `feat(game): one idle frame per 5 minutes; live views refresh within an idle frame`.
- [ ] **Step 7: V.**

### Task 4: cause events and dose preview (engine)

Spec 5.4: "After an action the toast says what happened and why when a rule fired". Causes are recorded where the rule fires: in the dose (`doses::apply`, at the end of the animation, R23) and at waking (`advance::wake`). They go into a transient queue that is never saved. `preview` runs the same code on a clone, so a risk marker is exactly what the dose would do.

**Files:**
- Create: `src/game/engine/causes.rs`.
- Modify: `src/game/engine/doses.rs` (`eat` 397, `sip` 485, `exercise` 240, `heal` 296, `relax` 287, `fall_asleep` 326), `src/game/engine/advance.rs` (collapse 185 to 192, `wake` 242), `src/game/engine/regime.rs` (`sleep_quality_pm` 47), `src/game/engine/mod.rs` (transient fields 350 to 367, `new_egg`, `from_bytes`, the methods below).
- Test: `tests/engine_v3.rs` `mod causes`.

**Interfaces:**
- Consumes: `doses::{apply, fall_asleep, wake_up}`, `advance::wake`, `needs::*`, Task 1 `SORE_PCT`.
- Produces:
```rust
// causes.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrainWhy { Hungry, Thirsty, Tired, Heavy, WornOut, Overdone }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unsettled { Drinks, Hungry, Stuffed, Thirsty, Waterlogged, Wired, Withdrawal, Unknown }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause { FoodComa, TreatTooSoon, NoTreatBuzz, DrinkDulled, OneTooMany, DryMouth, Relapse, Sloshing,
    TornMuscle(StrainWhy), Strained(StrainWhy), Grumpy, BadNight(Unsettled), Groggy, WornOut, StillHungOver,
    OnlyNumbed, OverRelaxed, WellRested }
impl Cause {
    pub fn what(self) -> &'static str;   // <= 12 cols
    pub fn why(self) -> &'static str;    // <= 15 cols
    pub fn is_warning(self) -> bool;     // false only for WellRested
    pub fn pack(self) -> u8; pub fn unpack(v: u8) -> Option<Cause>;
    pub fn all() -> impl Iterator<Item = Cause>;   // every variant incl. payloads, for width tests
}
pub const CAUSE_QUEUE: usize = 4;
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CauseQueue(heapless::Deque<Cause, CAUSE_QUEUE>);
impl CauseQueue { pub fn push(&mut self, c: Cause); /* full: drop the oldest */ pub fn iter(&self) -> impl Iterator<Item = &Cause>; pub fn is_empty(&self) -> bool; }
// regime.rs
pub fn unsettled_by(s: &GameState) -> Option<Unsettled>;   // sleep_quality_pm == poor  <=>  unsettled_by(s).is_some()
// mod.rs, GameState transient fields (not in to_bytes; from_bytes and new_egg start them empty / None)
pub causes: causes::CauseQueue,
pub bed_why: Option<causes::Unsettled>,
pub fn take_causes(&mut self) -> causes::CauseQueue;
/// First warning cause the action would produce if started now (a care action on a sleeper includes the wake).
/// Pure: works on a clone; None when the starter would refuse.
pub fn preview(&self, a: Action, food: Option<FoodKind>, drink: Option<DrinkKind>) -> Option<causes::Cause>;
pub fn preview_sleep(&self) -> Option<causes::Cause>;  // BadNight(unsettled_by) if unsettled; else Groggy if energy >= NEED_HI_EDGE_PCT (85); else None
pub fn preview_wake(&self) -> Option<causes::Cause>;   // first warning cause of wake_up on a clone; None if awake
```

**Emission rules.** "Before" means the state when the dose lands, as the existing multipliers read it.

| Where | Cause |
|---|---|
| `eat` | `FoodComa` if `excess > 0`. For a treat: `NoTreatBuzz` if `ozempic_min > 0`, else `TreatTooSoon` if `junk_min > 0` before |
| `sip`, alcoholic | `Relapse` if `rehab_min > 0`; `DrinkDulled` if `dim < 50` (the `dim` already computed); `DryMouth` if hydration before < `water_lo()`; `OneTooMany` if booze crosses `booze_hi()` upward |
| `sip`, any drink | `Sloshing` if hydration crosses `water_hi()` upward or fullness crosses `full_hi()` upward |
| `exercise` | `TornMuscle(why)` if injured by this session; else `Strained(why)` if any need, heavy or deprived multiplier applied. `why` is the first that held: Hungry (fullness < `full_lo`), Thirsty (hydration < `water_lo`), Tired (energy < `energy_lo`), Heavy (weight > `weight_hi`), WornOut (`deprived`), else Overdone |
| `heal` | `OnlyNumbed` if not injured and health > `DOSE_HEAL_HEALTHY_PCT` |
| `relax` | `OverRelaxed` if fun crosses `fun_lo()` downward |
| `fall_asleep` and collapse | set `bed_why = unsettled_by(s)` (no cause) |
| `advance::wake` | in this order: `Grumpy` (the existing grumpy rule fired); `WornOut` (`deprived` turned on in this wake); `Groggy` (groggy set in this wake); `BadNight(bed_why.or(unsettled_by(s)).unwrap_or(Unknown))` when `poor >= RESTLESS_MIN && poor * 2 > sleep_min - oversleep_min`, where `poor = sleep_min - oversleep_min - good_sleep_min` (saturating); `StillHungOver` if `hangover_min > 0`; `WellRested` if `rested_min` was set in this wake. Then `bed_why = None` |

`const RESTLESS_MIN: u16 = 60` in `advance.rs`. Unsettled is checked in the order Drinks (booze over `UNSETTLED_BOOZE_PCT`), Hungry, Stuffed, Thirsty, Waterlogged, Wired, Withdrawal (`rehab_min > 0`), using the predicates `sleep_quality_pm` uses today.

Causes are pushed in the same order on both drivers, so the full-state equality tests (R21, the oracle, split invariance) keep passing unchanged. Run them; if one fails, the emission is not deterministic, so fix the emission, not the test.

- [ ] **Step 1: write the failing tests** in `tests/engine_v3.rs` `mod causes` (Classic, an awake active pet). Each test applies the dose through `start` + `update` past the action end, then reads `take_causes()`.
```rust
#[test] fn pizza_on_a_half_full_pet_is_a_food_coma() { /* fullness 50 -> [FoodComa] */ }
#[test] fn a_treat_too_soon_or_on_ozempic() { /* junk_min 10, Cake -> contains TreatTooSoon; ozempic_min 10 -> NoTreatBuzz */ }
#[test] fn beer_on_a_thirsty_pet_dries_the_mouth() { /* hydration 30 -> contains DryMouth */ }
#[test] fn beer_past_the_band_is_one_too_many() { /* booze 30 -> contains OneTooMany */ }
#[test] fn a_drunk_pet_barely_feels_a_drink() { /* booze 60 -> contains DrinkDulled */ }
#[test] fn beer_in_rehab_is_a_relapse() { /* rehab_min 100 -> contains Relapse */ }
#[test] fn water_past_the_band_sloshes() { /* hydration 70 -> [Sloshing]; hydration 40 -> [] */ }
#[test] fn hungry_sport_strains_then_tears() { /* fullness 30: 1st session [Strained(Hungry)], 2nd [TornMuscle(Hungry)] */ }
#[test] fn medicine_on_a_healthy_pet_only_numbs() { /* health 90, not injured -> [OnlyNumbed] */ }
#[test] fn relaxing_a_low_fun_pet_bores_it() { /* fun 50 -> [OverRelaxed] */ }
#[test] fn woken_tired_is_grumpy() { /* asleep, energy 30, wake_up -> first == Grumpy */ }
#[test] fn a_night_after_drinks_is_a_bad_night() { /* booze 35, fall_asleep, advance 300 min, wake -> contains BadNight(Drinks) */ }
#[test] fn a_good_night_is_well_rested() { /* energy 30, fall_asleep, advance 400, wake -> contains WellRested, !WellRested.is_warning() */ }
#[test] fn the_queue_keeps_the_newest_four() {}
#[test] fn causes_are_never_saved() { /* to_bytes equal with and without queued causes; from_bytes -> empty queue, bed_why None */ }
#[test] fn preview_matches_what_the_dose_does() {
    // states: fresh; fullness 50; hydration 30 + booze 30; strain 40 + fullness 30; asleep energy 30;
    // health 90; fun 50; junk_min 10. For every Action x food x drink the starter accepts:
    // preview(..) == the first is_warning() cause of really starting and finishing it
}
#[test] fn preview_never_changes_the_pet() { /* clone before == state after preview / preview_sleep / preview_wake */ }
#[test] fn cause_text_fits_the_toast() { /* Cause::all(): what <= 12, why <= 15, pack/unpack round trip */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo test -j 2 --features simulator --test engine_v3 causes`.
- [ ] **Step 3: implement** following the rules table. Cause texts: see "On-screen words".
- [ ] **Step 4: run them and see them pass.** Then run the whole `--test engine_v3` and `--test engine_v3_casual` locally (the oracle and equality suites): PASS.
- [ ] **Step 5: commit** `feat(engine): cause events and pure dose preview`.
- [ ] **Step 6: V.**

### Task 5: cause toasts and the diary

**Files:**
- Create: `src/game/diary.rs`, `src/game/diary_view.rs`.
- Modify: `src/game/lifecycle.rs` (`cycle` 387, `start_new_game`, `new_generation`), `src/game/mod.rs` (`Toast` 56, toast draw 677, `live_view_open`), `src/game/modal.rs` (Stats items 91, `Item`), `src/game/input.rs`, `src/lib.rs` (view dispatch).
- Modify: `GAME.md` ("Stats" menu and "Actions": the toasts teach the rules).
- Test: `src/game/diary.rs` tests, `src/game/modal.rs` tests.

**Interfaces:**
- Consumes: Task 4 `Cause`, `GameState::take_causes`; Task 3 `live_view_open`.
- Produces:
```rust
// diary.rs
pub const DIARY_LEN: usize = 6;
#[derive(Clone, Copy, PartialEq, Debug)] pub struct Entry { pub cause: Cause, pub at_min: u32 }   // pet age in minutes
#[derive(Clone, Default)] pub struct Diary { /* heapless::Deque<Entry, DIARY_LEN> */ }
impl Diary { pub fn push(&mut self, cause: Cause, at_min: u32); pub fn newest_first(&self) -> impl Iterator<Item = &Entry>; pub fn len(&self) -> usize; pub fn clear(&mut self); }
pub fn fmt_age(min: u32) -> heapless::String<4>;   // "now" (<1), "59m", "23h", "9d", "99d" (capped)
pub fn entry_lines(e: &Entry, now_min: u32) -> (heapless::String<21>, heapless::String<21>);
    // ("{age:>3} {what}", "    {why}")
// diary_view.rs (full-screen, like traits_view): two pages of 3 entries
pub fn open(); pub fn close(); pub fn is_active() -> bool; pub fn flip(); pub fn page() -> u8;
pub fn draw<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error>;
// lifecycle.rs
pub fn diary() -> Diary;           // copy for the view
// mod.rs
Toast::Cause = 30;                  // two lines: what, why (from a CAUSE_CODE AtomicU8)
pub fn show_cause(c: Cause);
```
- `cycle()`, after `stats`, drains `take_causes()`. It pushes each cause into the diary with `at_min = age_ticks / 6`, then `show_cause(first)` if the drain was non-empty. The cause toast replaces a start toast that is still up. No buzz.
- `start_new_game` and `new_generation` clear the diary.
- Diary view:
  - title `"Diary {p}/2"`;
  - entries newest first, 3 per page;
  - line 1 bold at y = 22 + 40 i, line 2 regular 13 px below;
  - empty diary: `"Nothing yet"` centred;
  - Left / Right flips; any other button closes;
  - `live_view_open()` also covers `diary_view::is_active()` (the ages change).
- The Stats menu becomes `[View stats, Diary, Rolled stats, Health status, Friends, Cancel]`. Task 7 removes Health status and Task 9 adds Hint.

- [ ] **Step 1: write the failing tests.**
```rust
#[test] fn ages_read_at_a_glance() { /* 0 "now", 59 "59m", 60 "1h", 1439 "23h", 1440 "1d", 999_999 "99d" */ }
#[test] fn the_diary_keeps_the_last_six_newest_first() {}
#[test] fn diary_lines_fit_the_screen() { /* every Cause::all() at age 99d: both lines <= 21 cols */ }
#[test] fn an_action_end_shows_its_cause_and_logs_it() {
    // lock_test_state; active, fullness 70; Item::FeedFood(Pizza).activate(); close; debug_skip_minutes(5); cycle()
    // -> current toast is Toast::Cause with FoodComa; diary().len() == 1
}
#[test] fn a_new_pet_starts_an_empty_diary() {}
#[test] fn diary_pages_flip_with_left_right() { /* open; Right -> page 1; Right -> page 0; Fire closes */ }
#[test] fn stats_menu_has_a_diary_row() {}
#[test] fn an_open_diary_refreshes_within_an_idle_frame() { /* next_ui_deadline_tick() <= now + 30 */ }
```
- [ ] **Step 2: run and see them fail.** `... --lib diary` and `... --lib modal`.
- [ ] **Step 3: implement.**
- [ ] **Step 4: update the docs.** In `GAME.md`: the cause toasts (what fired and why), and the Diary under Stats (last 6, ages, two pages, kept until reboot).
- [ ] **Step 5: run them and see them pass.**
- [ ] **Step 6: commit** `feat(game): cause toasts and the diary`.
- [ ] **Step 7: V.**

### Task 6: two-page stats view with band ticks

Spec 5.1, approved by the owner.
- Page 1 "Needs": Full, Water, Energy, Fun, Clean.
- Page 2 "Body": Health, Fit, Weight, Booze, Mood, then the condition line.
- Today's geometry: 132 x 132 popover, 15 px bars at a 15 px pitch, a 60 px label column, a 60 px bar, and the footer.
- Left / Right flips the page; the title shows `1/2` or `2/2`; one partial refresh per flip (no `FULL_REFRESH_PENDING`).
- Fire and Cancel behave as today.
- The fill is black when the value is inside its band and red outside it.

**Files:**
- Create: `src/game/stats_pages.rs`.
- Modify: `src/game/stat_bar.rs`, `src/game/modal.rs` (`draw_stats_view` 1047, `move_cursor`, `activate`, the `STATS_VIEW` handling), `src/game/input.rs` (modal branch 141), `src/game/engine/mod.rs` (`PetStats`, `stats`).
- Modify: `GAME.md` ("Stats"), `USER_GAMES.md` ("Stats (the four things to watch)": rewrite for the two pages).
- Test: `src/game/stats_pages.rs` tests, `src/game/stat_bar.rs` tests, `src/game/modal.rs` tests, `tests/engine_v3_casual.rs`.

**Interfaces:**
- Consumes: Task 1 `Conditions`, `conditions`, `Conditions::line`; `regime::health_ceiling`; `needs::*`; `t::MOOD_HEALTH_FLOOR_PCT`, `t::DMG_MOOD_PCT`.
- Produces:
```rust
// PetStats gains
pub ceiling: u8, pub conditions: engine::status::Conditions, pub line: engine::status::StatusLine,
// stats_pages.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum Page { Needs, Body }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BarRow { pub label: &'static str, pub pct: u8, pub lo: Option<u8>, pub hi: Option<u8>, pub cap: Option<u8>, pub red: bool }
pub fn bars(page: Page, s: &PetStats) -> [BarRow; 5];
pub fn title(page: Page) -> &'static str;   // "Needs 1/2", "Body 2/2"
// stat_bar.rs
pub fn tick_x(bar_x: i32, bar_w: u32, pct: u8) -> i32;   // bar_x + 1 + pct * (bar_w - 2) / 100
pub fn draw_band_bar<D: DrawTarget<Color = TriColor>>(display: &mut D, row: &BarRow, label_pos: Point, bar_origin: Point, bar_size: Size) -> Result<(), D::Error>;
// modal.rs
pub fn flip_page();   // only while the stats view shows; Needs <-> Body
```
The bar table (Classic numbers in the tests come from these accessors):

| Page | Label | pct | lo | hi | cap | red when |
|---|---|---|---|---|---|---|
| Needs | Full | fullness | `FULL_LO` | `FULL_HI` | – | outside |
| Needs | Water | hydration | `WATER_LO` | `WATER_HI` | – | outside |
| Needs | Energy | energy | `ENERGY_LO` | – | – | below |
| Needs | Fun | fun | `FUN_LO` | `FUN_HI` | – | outside |
| Needs | Clean | 100 - dirt | 100 - `DIRT_HI` | – | – | below |
| Body | Health | health | `MOOD_HEALTH_FLOOR_PCT` | – | ceiling | below lo |
| Body | Fit | fitness | `FIT_LO` | – | – | below |
| Body | Weight | weight | `WEIGHT_LO` | `WEIGHT_HI` | – | outside |
| Body | Booze | booze | – | `BOOZE_HI` | – | above |
| Body | Mood | mood | – | – | – | below `DMG_MOOD_PCT` |

Ticks are drawn with `tick_x`. Each is a 1 px line across the inner height, WHITE over the fill and BLACK over the empty part, plus a 3 px black notch on the top and bottom border rows. `cap` is drawn the same way and never turns the fill red. The inline percentage stays as `draw_stat_bar` draws it today. `draw_stat_bar` stays for `traits_view`.

Page 2 has the condition row at y = inner_y + TITLE_H + 2 + 5 * 15 + 1, drawn as `TEXT_BLACK` with `conditions.line(18)`. The footer is the same on both pages. Opening the stats view always starts on page 1.

- [ ] **Step 1: write the failing tests.**
```rust
#[test] fn needs_page_rows_and_bands() {
    // fresh active pet: labels [Full, Water, Energy, Fun, Clean]; Full lo 38 hi 78; Water 38/83; Energy lo 33 hi None;
    // Fun 33/73; Clean lo 42; no row red
}
#[test] fn body_page_marks_what_is_out() {
    // weight 70 -> Weight red; booze 45 -> Booze red; fitness 30 -> Fit red; health 45 -> Health red;
    // fitness 60 -> Health cap Some(80); mood 15 -> Mood red; every label <= 8 chars
}
#[test] fn tick_x_maps_percent_to_the_inner_bar() { /* tick_x(76, 60, 0) == 77; tick_x(76, 60, 100) == 135; tick_x(76, 60, 50) == 106 */ }
#[test] fn band_bar_draws_ticks_and_red_fill() {
    // Canvas; row Weight pct 70 lo 38 hi 63 red: RED inside the fill; BLACK notch at (tick_x(.., 63), bar top row)
}
#[test] fn left_right_flips_the_stats_pages() {
    // lock_test_state; open Stats, ViewStats: page Needs; flip_page -> Body; flip_page -> Needs;
    // activate() -> back to the list; reopen -> Needs; flip_page with the list showing -> no change
}
// tests/engine_v3_casual.rs (serialized install)
#[test] fn casual_bands_move_the_ticks() { /* Casual: Full lo 33 hi 83; Fit lo 38 */ }
```
- [ ] **Step 2: run and see them fail.** `... --lib stats_pages`, `... --lib stat_bar`, `... --lib modal`, `... --test engine_v3_casual casual_bands`.
- [ ] **Step 3: implement.** In `input.rs` the modal branch sends `Left | Right` to `modal::flip_page()`.
- [ ] **Step 4: update the docs.** `GAME.md` "Stats": the two pages, what each bar means, ticks, red, the condition words, Left / Right. Remove the v1 "lower is better / six stats" table. `USER_GAMES.md` "Stats": the same in short.
- [ ] **Step 5: run them and see them pass.** Run `cargo sim` once: flip both pages.
- [ ] **Step 6: commit** `feat(game): two-page stats view with comfort-band ticks`.
- [ ] **Step 7: V.**

### Task 7: Health status entry removed; the battle record moves to the Battle screen

Spec 5.1 and 17 row 32: page 2 covers the conditions, Fit and Booze. The battle record moves to the Friends / Battle screen. "Neglect -> permanent" moves to the docs.

**Files:**
- Delete: `src/game/health_view.rs`.
- Modify: `src/game/mod.rs` (`pub mod health_view`), `src/game/modal.rs` (`Item::HealthStatus`), `src/game/input.rs` (31 to 36), `src/lib.rs` (642 to 645), `src/game/battle_view.rs` (`draw_picker` 400), `src/game/lifecycle.rs`, `src/game/engine/mod.rs` (`PetStats` fields left without a reader).
- Modify: `README.md:26`, `GAME.md` (66, 281 and "Battles"), `tests/engine_v3.rs:2548` if a removed field is read there.
- Test: `src/game/modal.rs`, `src/game/battle_view.rs` tests.

**Interfaces:**
- Produces:
```rust
// battle_view.rs
pub fn picker_title(wins: u16, losses: u16) -> heapless::String<21>;   // "Battle {w}W-{l}L"
// lifecycle.rs
pub fn battle_record() -> Option<(u16, u16)>;
```
- `draw_picker` uses `picker_title` in place of "Battle a Friend", so the record shows with or without friends.
- R15: `PetStats.overweight` and `PetStats.drunk` go if `health_view` was their last reader (`battle.rs` still reads `healthy` and `alcoholic`, which stay).

- [ ] **Step 1: write the failing tests.**
```rust
#[test] fn stats_menu_has_no_health_status_row() { /* labels == ["View stats", "Diary", "Rolled stats", "Friends", "Cancel"] */ }
#[test] fn the_battle_picker_shows_your_record() { /* picker_title(3, 1) == "Battle 3W-1L"; picker_title(9999, 9999).len() <= 21 */ }
```
- [ ] **Step 2: run and see them fail.** `... --lib modal` and `... --lib battle_view`.
- [ ] **Step 3: implement** the removal and the title.
- [ ] **Step 4: update the docs.**
  - `README.md:26`: the Health Status line becomes the two-page stats view.
  - `GAME.md`: the Stats menu list; the Battles section says where your record shows.
  - A "Permanent conditions" paragraph carries the "Neglect -> permanent" meaning: diabetes and dependence stay until treated, as in spec 15.4.
- [ ] **Step 5: run them and see them pass.** `grep -rn health_view src` returns nothing.
- [ ] **Step 6: commit** `feat(game): drop the Health status screen; battle record on the Battle screen`.
- [ ] **Step 7: V.** In the report, note the flash size drop from the removed view.

### Task 8: menu risk markers

Spec 5.5: the row is still usable; the hint is a warning, not a lock. A suffix such as `Frikandel spec 10H (full)` would be 25 columns, so the 17-column row cannot hold it. Instead:
- a row whose action would trigger a warning cause gets the marker `" !"` after its price;
- while the cursor is on such a row, the modal title bar reads `"risk: " + cause.what()` in place of the menu title.

The prediction is `preview`, so the marker is exactly what the dose would do.

**Files:**
- Modify: `src/game/modal.rs` (`draw_modal` 789, `Item`), `src/game/lifecycle.rs`, `src/game/engine/food.rs` (`label` keeps `Frikandel spec`; full name `Frikandel speciaal` in the selected-row title — owner ruling, see ledger).
- Modify: `GAME.md` ("Actions": the `!` and the risk title; the Frikandel name).
- Test: `src/game/modal.rs` tests.

**Interfaces:**
- Consumes: Task 4 `GameState::{preview, preview_sleep, preview_wake}`, `Cause::{what, is_warning}`.
- Produces:
```rust
// lifecycle.rs (read-only wrappers over GAME)
pub fn preview(a: Action, food: Option<FoodKind>, drink: Option<DrinkKind>) -> Option<Cause>;
pub fn preview_sleep() -> Option<Cause>;
pub fn preview_wake() -> Option<Cause>;
// modal.rs
impl Item { fn risk(self, stats: &PetStats) -> Option<Cause>; }   // None for unavailable rows and non-action rows
fn row_text(item: Item, stats: Option<&PetStats>) -> heapless::String<40>;   // label + price + cooldown + " !"
fn title_text(kind: ModalKind, selected: Option<Item>, stats: Option<&PetStats>) -> heapless::String<24>;
```
- `risk` maps:
  - `FeedFood(f)` → `preview(Feed, Some(f), None)`;
  - `DrinkChoice(d)` → `preview(Drink, None, Some(d))`;
  - the other action rows → `preview(action, None, None)`;
  - `Sleep` → `preview_sleep()`;
  - `WakeFromSleep` → `preview_wake()`.
- Only warning causes count.

- [ ] **Step 1: write the failing tests.**
```rust
#[test] fn a_half_full_pet_marks_pizza_with_the_food_coma() {
    // fullness 50: Pizza row ends " !", Salad and Cake do not; title with Pizza selected == "risk: food coma"
}
#[test] fn exercise_row_warns_before_the_tear() { /* strain 40, fullness 30: risk == TornMuscle(Hungry) */ }
#[test] fn wake_row_warns_grumpy_under_fifty() { /* asleep energy 30: WakeFromSleep risk == Grumpy */ }
#[test] fn sleep_row_warns_after_drinks() { /* booze 35: Sleep risk == BadNight(Drinks) */ }
#[test] fn marked_rows_still_work() { /* fullness 50: Pizza available, activate starts Feed */ }
#[test] fn rows_and_risk_titles_fit() {
    // every row of Feed, Drink, Heal, Play, Rest, Exercise with money on, normal and hard mode, plus " !": <= 17 cols;
    // "risk: " + what for every warning Cause: <= 18 cols
}
```
- [ ] **Step 2: run and see them fail.** `... --lib modal`.
- [ ] **Step 3: implement.** `draw_modal` uses `row_text` and `title_text`.
- [ ] **Step 4: update the docs.** `GAME.md` "Actions": the `!` marker, the risk title, the full name `Frikandel speciaal`. Remove the v1 "(wait)" cooldown paragraph that plan 1 left.
- [ ] **Step 5: run them and see them pass.**
- [ ] **Step 6: commit** `feat(game): menu rows warn what a dose would cause`.
- [ ] **Step 7: V.**

### Task 9: "Hint": the pet says what it needs

Owner (2026-11-13), verbatim: "What would help is also a hint, for example 'I'm hungry please feed me', or 'I'm miserable play with me' those kinds of things, an option with 'hint' in the stats menu?"

Controller rulings:
- one table, derived from the status line;
- danger first;
- never suggest an action the engine refuses: use `refusal_for` and the same price check as the menu;
- conditions get their real remedy;
- egg, gone and hibernating get their own lines;
- a worst-case width test.

The subject is the hint's status-line word: `status_line(s).first` (danger word, then the largest penalty, then the prompt), with Egg, Gone and Hibernating in front. When `first` is only an info word, `WornOut` (sleep-deprived) wins over it. Line 1 names the subject. Line 2 is the ask of the first remedy the pet would accept, or `"but " + Reason::text()` of the first remedy when all are refused, or the subject's fixed line when it has no remedy.

The hint is shown in a popover view inside the Stats modal, titled `"<name> says"` (or `"Your pet says"` when unnamed). It stays until any button; Fire returns to the list. It is a live view (Task 3).

**Files:**
- Create: `src/game/engine/hint.rs`.
- Modify: `src/game/engine/doses.rs` (`price`), `src/game/engine/mod.rs` (`remedy_reason`), `src/game/modal.rs` (`Item::Hint`, hint view, `Item::hax_price` delegates to `doses::price`), `src/game/mod.rs` (`live_view_open`).
- Modify: `GAME.md` (Stats menu: Hint).
- Test: `tests/engine_v3.rs` `mod hint`, `src/game/modal.rs` tests.

**Interfaces:**
- Consumes: Task 1 `status_line`, `Word`, `conditions`; `GameState::{refusal_for, sleep_refusal, game_refusal, can_afford}`; `doses::Reason::text`.
- Produces:
```rust
// doses.rs: the one price table (starters, menu and hint read it)
pub fn price(a: Action, food: Option<FoodKind>, drink: Option<DrinkKind>, hard: bool) -> Option<u32>;
// hint.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Remedy { Feed, Water, Sleep, Play, Game, Relax, Wash, Exercise, Medicine, Insulin, Rehab, Defrost }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subject { Word(Word), WornOut, Egg, Gone, Hibernating }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask { Do(Remedy), Refused(Reason), Say(&'static str) }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hint { pub subject: Subject, pub ask: Ask }
pub fn subject_line(subject: Subject) -> &'static str;          // line 1
pub fn remedies(subject: Subject) -> &'static [Remedy];         // the table, in preference order
pub fn otherwise(subject: Subject) -> &'static str;             // line 2 when remedies() is empty
impl Remedy { pub fn ask(self) -> &'static str; }               // line 2 when accepted
impl Hint { pub fn lines(&self) -> (heapless::String<18>, heapless::String<18>); }
pub fn hint(s: &GameState) -> Hint;
// mod.rs
impl GameState { pub fn remedy_reason(&self, r: Remedy) -> Option<Reason>; }
```
`remedy_reason` uses the same checks as the matching menu rows, plus the price whenever `money_enabled` (`CantAfford` when unaffordable):

| Remedy | Check |
|---|---|
| Feed | `refusal_for(Feed, None)`, cheapest food |
| Water | `refusal_for(Drink, Some(Water))` |
| Sleep | `sleep_refusal()` |
| Play | `refusal_for(Play, None)` |
| Game | `None` if any mini-game's `game_refusal(Some(g))` is `None`, else the first one's reason |
| Relax, Wash, Exercise | `refusal_for(..)` |
| Medicine | `refusal_for(Heal, ..)` |
| Insulin | `refusal_for(Medicate, ..)` |
| Rehab | `refusal_for(Rehab, ..)` |
| Defrost | `None` while hibernating, else `Refused(NoNeed)` |

The table (remedies checked against `doses.rs`; lines in "On-screen words"):

| Subject | Remedies |
|---|---|
| Peckish, Hungry, Starving, Underweight | Feed |
| Thirsty, Parched | Water |
| Sloppy, Wasted | Water (water lowers booze) |
| Sleepy, Exhausted, WornOut | Sleep |
| Bored | Play, Game |
| Wired, Sore | Relax |
| Grubby, Filthy | Wash |
| Overweight, Unfit | Exercise |
| HungOver | Water, Sleep |
| Groggy | Water, Play |
| Injured, Poorly | Medicine |
| NoInsulin | Insulin |
| NeedsRehab | Rehab |
| Hibernating | Defrost |
| Stuffed, Waterlogged, Tipsy, Asleep, WellRested, Content, Egg, Gone | none (fixed line 2) |

- [ ] **Step 1: write the failing tests.** `tests/engine_v3.rs` `mod hint`:
```rust
#[test] fn every_word_and_condition_has_a_hint() {
    // every Word::ALL as Subject::Word, WornOut, Egg, Gone, Hibernating: subject_line non-empty;
    // remedies non-empty or otherwise non-empty; every Condition has a Word or WornOut subject that covers it
}
#[test] fn suggested_action_is_never_refused() {
    // states: each DebugPoke from Comfort; each again asleep; again with an action running; again with
    // money_enabled and money 0; injured + bored; med_cd_min 120 + injured; hard mode.
    // For each: match hint(&s).ask { Ask::Do(r) => assert_eq!(s.remedy_reason(r), None),
    //   Ask::Refused(why) => assert_eq!(Some(why), s.remedy_reason(remedies(subject)[0])), Ask::Say(_) => remedies empty }
}
#[test] fn the_danger_hint_wins() { /* dirt 86 + fun 25 awake: subject Word(Filthy), ask Do(Wash) */ }
#[test] fn hungry_but_busy_says_why() { /* fullness 20, Play running: lines == ("I'm hungry!", "but busy") */ }
#[test] fn an_injured_bored_pet_asks_for_a_game() { /* injured, fun 10 awake: ask Do(Game) */ }
#[test] fn medicine_on_cooldown_says_wait() { /* injured, med_cd_min 120: "but wait 2h00" */ }
#[test] fn egg_gone_and_hibernating_have_their_own_lines() {}
#[test] fn hint_lines_fit_the_popover() {
    // every subject x (every Remedy ask, every Reason incl. Wait(72 * 3600), otherwise): both lines <= 18 cols
}
```
`src/game/modal.rs` tests:
```rust
#[test] fn hint_and_menu_agree() {
    // same state list; Remedy -> its menu rows (Feed: any FeedFood; Water: DrinkChoice(Water); Sleep; PlayNow;
    // Game: the mini-game rows; Relax; Wash; ExerciseNow; GiveMedicine; GiveMedication; Rehab; Defrost: WakeUp):
    // remedy_reason(r).is_none() == rows.any(|i| i.available(&stats))
}
#[test] fn hint_row_opens_the_speech_view() { /* Stats items == ["View stats", "Hint", "Diary", "Rolled stats", "Friends", "Cancel"];
                                                activate Hint -> hint view shown; title "<name> says" <= 18 cols for a 12-char name;
                                                any button returns to the list */ }
```
- [ ] **Step 2: run and see them fail.** `... --test engine_v3 hint` and `... --lib modal`.
- [ ] **Step 3: implement** `price` (the starters and `Item::hax_price` call it; the prices do not change), `remedy_reason`, `hint.rs`, the row and the view.
- [ ] **Step 4: update the docs.** `GAME.md` Stats menu: "Hint: your pet tells you its most pressing need and what helps".
- [ ] **Step 5: run them and see them pass.**
- [ ] **Step 6: commit** `feat(game): Hint row, the pet says what it needs`.
- [ ] **Step 7: V.**

## On-screen words

Every new or renamed on-screen text in this plan, for the player's guide. Case as drawn.

- **Status line** (Task 1):
  - need words: `peckish`, `hungry`, `starving`, `stuffed`, `thirsty`, `parched`, `waterlogged`, `sleepy`, `exhausted`, `bored`, `wired`, `tipsy`, `sloppy`, `wasted`, `grubby`, `filthy`, `overweight`, `underweight`, `unfit`, `hung over`, `sore`, `injured`, `groggy`, `poorly`;
  - prompts: `no insulin`, `needs rehab`;
  - info words: `asleep`, `well rested`, `content`;
  - joiner: `, `. Red when in danger.
- **Conditions** (Task 1, stats page 2): `injured`, `sore`, `hung over`, `groggy`, `worn out`, `diabetic`, `dependent`, `in rehab`, `well rested`; `+N` for more.
- **Cause toasts and diary** (Task 4; what / why):

  | What | Why |
  |---|---|
  | `food coma` | `ate too much` |
  | `barely felt` | `treat too soon` |
  | `no buzz` | `on Ozempic` |
  | `barely felt` | `drunk already` |
  | `sloppy` | `one too many` |
  | `dry mouth` | `booze, no water` |
  | `relapse!` | `rehab lost` |
  | `sloshing` | `drank too much` |
  | `torn muscle` / `strained` | `sport: hungry`, `sport: thirsty`, `sport: tired`, `sport: heavy`, `sport: worn out`, `sport: too much` |
  | `grumpy` | `woken too early` |
  | `bad night` | `late drinks`, `hungry`, `stuffed`, `thirsty`, `waterlogged`, `wired`, `withdrawal`, `slept badly` |
  | `groggy` | `overslept` |
  | `worn out` | `short nights` |
  | `hung over` | `needs water` |
  | `only numbed` | `not sick` |
  | `bored` | `too much relax` |
  | `well rested` | `good sleep` |

- **Diary** (Task 5): `Diary 1/2`, `Diary 2/2`, `Nothing yet`; ages `now`, `Nm`, `Nh`, `Nd`.
- **Stats view** (Task 6): titles `Needs 1/2`, `Body 2/2`; labels `Full`, `Water`, `Energy`, `Fun`, `Clean`, `Health`, `Fit`, `Weight`, `Booze`, `Mood`.
- **Menus** (Tasks 5, 7, 8, 9):
  - Stats menu: `View stats`, `Hint`, `Diary`, `Rolled stats`, `Friends`, `Cancel` (`Health status` removed);
  - row marker ` !`; title `risk: <what>`;
  - food label `Frikandel spec` kept (Dutch snack-bar shorthand); full name `Frikandel speciaal` in titles and toasts;
  - Battle picker title `Battle <w>W-<l>L`.
- **Hint** (Task 9): title `<name> says` / `Your pet says`.
  - Line 1:

    | Subject | Line 1 |
    |---|---|
    | need and condition words | `I'm <word>!` (for example `I'm hungry!`, `I'm hung over!`) |
    | `no insulin` | `I'm diabetic!` |
    | `needs rehab` | `I'm dependent!` |
    | worn out | `I'm worn out!` |
    | asleep | `Zzz...` |
    | well rested | `I feel great!` |
    | tipsy | `I'm tipsy!` |
    | content | `I'm happy!` |
    | egg | `*wobble*` |
    | gone | `...` |
    | hibernating | `Brrr, frozen!` |

  - Line 2 asks:

    | Remedy | Ask |
    |---|---|
    | Feed | `Please feed me.` |
    | Water | `Water, please.` |
    | Sleep | `Put me to bed.` |
    | Play | `Play with me!` |
    | Game | `Play a game!` |
    | Relax | `Let me relax.` |
    | Wash | `Wash me, please.` |
    | Exercise | `Let's work out!` |
    | Medicine | `I need medicine.` |
    | Insulin | `I need insulin.` |
    | Rehab | `I need rehab.` |
    | Defrost | `Defrost me!` |

  - Refused: `but <reason>` with the R24 reasons: `busy`, `too tired`, `injured`, `asleep`, `not thirsty`, `already clean`, `no need`, `wait <t>`, `can't afford`, `hibernating`, `still an egg`, `pet is gone`.
  - Fixed line 2:

    | Subject | Line 2 |
    |---|---|
    | stuffed | `Let me digest.` |
    | waterlogged | `No more drinks.` |
    | tipsy | `Hic! Thanks!` |
    | asleep | `Let me sleep.` |
    | well rested, content | `Thanks!` |
    | egg | `I'm hatching!` |
    | gone | `Your pet left.` |

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| 5.1 two pages, ticks, red | T6 |
| 5.1 Health status removed, battle record moved | T7 |
| 5.1 care score in the page-2 footer | not done: roadmap row 8 owns the care score display, and plan 3 changes what it counts; the footer stays as on page 1 |
| 5.1 strain mark on the Fit bar | not done: redundant with the `sore` condition word |
| 5.2 status line | T1, T2 |
| 5.3 sprites | unchanged |
| 5.4 cause toasts | T4, T5 |
| 5.5 menu hints | T8 (marker + risk title) |
| 5.6 diary | T5 |
| 5.7 buzzer | unchanged; constraint |
| 17 row 29 | T3 |
| 17 row 32 | T7 |
| 18.6 refresh budget | T3: idle refreshes at most 288 a day; status changes stay engine wakes |
| Owner hint request | T9 |
| Conditions `uninsured` / `overdue` | plan 6 adds two `Condition` variants; `line()` already handles any count |

**Resolved conflicts and gaps (proposed rulings):**
1. **Corner banners.** "Replaces the corner banners": NEEDS MEDS and NEEDS REHAB become the prompt words `no insulin` and `needs rehab` in the second slot. OVERWEIGHT and DRUNK become the status words `overweight` and `sloppy`.
2. **Status line position.** "Under the pet" has no free pixels (the sprite is 152 x 75), so the line sits at the bottom of the pet area on a white backing, 1 or 2 lines.
3. **Tipsy and sloppy.** The spec 5.2 vocabulary has `tipsy` but no `sloppy`, while 15.4 defines tipsy (10 to 40 %) and sloppy (over 40 %). Ruling: `tipsy` is an info word, `sloppy` is the penalty word, and `wasted` (new) is the danger grade.
4. **Menu hint suffix.** The 5.5 suffix does not fit 17 columns, so rows get ` !` and the title shows `risk: <what>`. `Water (not thirsty)` and `Wash (clean)` are refusals since plan 1 row 8 and use the R24 reason. `Beer (3rd today)` has no counter in v3; the tolerance rule it teaches is `barely felt / drunk already` and `one too many`, judged on booze level as the rule is.
5. **Cause list.** `annoyed: not dirty` is obsolete (the wash is refused). Added: `dry mouth`, `relapse!`, `sloshing`, `strained`, `only numbed`, `bored / too much relax`, `worn out`, `well rested`. `restless night: late beer` needs the bedtime reason, so it uses the transient `bed_why`.
6. **Diary size.** Six two-line entries do not fit one screen, so the diary has two pages of three, with the same Left / Right as the stats view.
7. **Drifting numbers.** The stats, diary and hint views show numbers that drift; the owner rule is met by bounding them with the UI deadline at one idle frame (5 min), not by per-percent wakes.
8. **Mood bar.** Mood has no band in the spec mock-up. Ruling: no ticks; red below the health-damage line `DMG_MOOD_PCT`.
9. **CI coverage.** CI built only the SSD1675 firmware. Task 1 adds the SSD1680 build and a 960 KiB flash gate.

**Needs the owner:**
- (a) The new display constants (`IDLE_FRAME_SECS` 300, `SECOND_WORD_UNDER` 10 points, `RESTLESS_MIN` 60, `DIARY_LEN` 6) are consts, not `Thresholds` fields: they are not balance. Say so if they should be `BPETS_V3.CFG` keys.
- (b) The care score stays out of the page-2 footer until plan 8.
- (c) The menu-hint form: ` !` plus `risk:` title instead of a suffix.
- (d) The four corner banners are removed.
- (e) Owner: keep `Frikandel spec` — it is the Dutch snack `Frikandel speciaal`, a nod to the badge design's Dutch origin; show the full name where width allows.
- (f) The strain mark on the Fit bar is dropped.

**Type consistency:** checked `StatusLine`, `Word`, `Conditions`, `Cause`, `CauseQueue`, `Remedy`, `Subject`, `Ask`, `preview*`, `remedy_reason`, `price`, `live_view_open`, `IDLE_FRAME_TICKS` across the tasks that consume them. The Stats item lists are pinned per task: T5 adds Diary, T7 removes Health status, T9 adds Hint, and each test states the list at that point.

**Review Focus:** items 1 to 5 are pinned in T1, T3, T9, T4, and T1 / T5 / T7 / T8 / T9.

**Flash:** no font, no sprite. New tables and strings are estimated at under 8 KB. `health_view.rs` and its primitive icons go. CI prints the SSD1680 release size each task. If it exceeds 960 KiB, Lights Out is removed (owner's allowance) in its own commit.
