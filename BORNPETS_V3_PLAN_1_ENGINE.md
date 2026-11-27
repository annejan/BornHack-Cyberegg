# BornPets v3, plan 1: engine core and save v3

**Goal:** replace the v1 rules inside the existing delta engine with the v3 comfort-band model (needs, doses, interactions, conditions, mood chase, health with fitness ceiling, sleep without a timer), keep saves loadable, and prove it against a step oracle in the simulator.

**Architecture:** `GameState::update()` keeps its shape (catch up the elapsed time in segments). The segment length comes from a table-driven solver; one `advance(dt)` applies the regime's constant rates in closed form. Time inside the engine is whole minutes (1 minute = 6 ticks) with the tick remainder carried. The one-minute step loop exists only as a simulator driver for tests.

**Tech stack:** Rust, `no_std` in firmware, integer math only. Host tests through the `simulator` feature.

**Spec:** `BORNPETS_V3_DESIGN.md` (repo root). Authoritative sections: 13 (delta engine), 15.1 to 15.6 and 15.8 (stats, weight, booze, timescales, remedies), 17 and 18 (owner decisions and applied values), 18.8. Reference implementation of every rule in this plan: `prototypes/bornpets-simv3/src/main.rs` (`rates`, `thresholds`, `seg_len`, `sig`, `advance`, `wake`, `act`).

**Code base:** branch `ranzbak/bornpets-v2` (cleanup `710371f` + main `9e22fb8`; anchors taken at `710371f`, `modal.rs` may have shifted with #25). Anchors: `src/game/engine/mod.rs` `GameState` 204, `update` 540, `ticks_to_next_rate_change` 646, `consume_action_ticks` 832, `check_leaving` 1091, `start_action` 1170, `next_wake_tick` 1436, `PetStats` 1571, `stats` 1717, `SAVE_FORMAT_VERSION` 1902, `to_bytes` 1910, `from_bytes` 2003; `src/game/engine/sim.rs` `Policy` 98, `run` 136; `src/game/lifecycle.rs` `try_load` 113; `src/game/modal.rs` Heal list 108, Rest list 176; `tests/save_format.rs`, `tests/game_engine.rs`.

## Global Constraints

- The delta engine drives progress: state advances only inside `update()` by closed-form segments.
- No fixed-step loop and no polling on the badge. `advance(1)` in a loop is allowed only under `#[cfg(feature = "simulator")]`.
- Integer math only: no `f32`/`f64` in `src/game/engine/` outside `sim.rs`.
- `no_std`: no `Vec`, no `alloc` in engine code outside `sim.rs`.
- Save compatibility: a save-format-2 save (97 bytes, version byte 2) must load as a living v3 pet; never reset.
- Scale: levels are `u16`, `P = 655` is one percent, `MAX = 65535`.
- 1 engine minute = 6 ticks; 1 tick = 10 s.
- Engine wake cap: 720 ticks (2 h).
- **v3 balance is runtime-tunable** (owner, 2026-11-13: "make the new engine tunable as well"; revised: "we don't rename the current config to .bak, we create a new config file with v2 in the name, that way players can revert back to the old firmware if they want to", then "BPETS_V3.CFG"). Every v3 rate, band edge, dose, timer and condition threshold is a field of the existing `Thresholds` table (`src/game/engine/thresholds.rs`, `thresholds_table!`) with a Classic and a Casual value and a `BORNPETS_CFG_KEYS` row. The v3 firmware reads `BPETS_V3.CFG`; the old `BORNPETS.CFG` is never read, written, renamed or deleted by v3 or by the update script, so the old firmware keeps its file. Engine code reads the values through the generated accessors, never as literals. Numbers pinned in Tasks 3 to 10 are the **Classic** values; tests run with the Classic preset installed and may use literals.
- Casual preset (spec 10, phase 2): every band 5 points wider on each side (lower edges -5, upper edges +5, clamped to 1..99), every health-damage rate halved; all other values equal to Classic.
- No v3 config key reuses a v1 key name; v1 keys left in an old file are unknown and skipped.
- Fixed safety edges (15, 25, 85, 90, 95 and the other non-band entries of the edge table) stay `const`; only band edges and the danger lines named in Task 1 are tunable.
- Local builds and tests use at most 2 cores: prefix `CARGO_BUILD_JOBS=2 taskset -c 0,1` and pass `-j 2`.
- Test command: `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo test -j 2 --features simulator --test <file> <name>`.
- Firmware check: `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo build -j 2 --bin embassy --no-default-features --features embassy-game --target thumbv7em-none-eabihf`.

## Review Focus

Failure modes the spec implies and that are easy to ship unnoticed, most likely first. Each is pinned by a test in the task named.

1. **Tick remainder lost.** `update()` is called every few ticks by the display loop; if elapsed ticks are divided by 6 and the remainder dropped, a watched pet never ages. Pinned in Task 6 (`many_small_updates_equal_one_big_update`).
2. **A segment runs past a threshold** because a rate was taken from the regime at the start and a level crossed mid-segment (most visible in long sleeps and multi-day catch-ups). Pinned in Task 12 (`delta_matches_oracle_all_routines`, `fourteen_day_gap_matches_oracle`).
3. **Mood or health crosses its own line inside a segment**, so health keeps the wrong recovery rate for hours. Pinned in Task 8 (`mood_crossing_60_splits_the_segment`).
4. **The save format 2 upgrade maps the old bad-high stats one to one**, giving stuffed, exhausted or unfit pets after the update. Pinned in Task 11 (`v2_golden_upgrades_to_expected_v3_levels`).
5. **`next_wake_tick` is later than the first visible change, or always at the cap.** Pinned in Task 10 (`wake_is_never_later_than_first_status_change`, `wake_budget_balanced_day`).
6. **A hand-edited `BPETS_V3.CFG` breaks the solver or the pet** (lower edge above upper edge, a zero rate used as a divisor, an edge of 0 or 100). Pinned in Task 1 (`bad_band_pair_falls_back_to_preset`, `zero_rate_keys_are_floored`) and Task 15 (`template_parses_to_classic_with_no_edits`).

## File map

| File | Change |
|---|---|
| `src/game/engine/needs.rs` | create: `Need`, band accessors, edge table built from the active `Thresholds` |
| `src/game/engine/thresholds.rs` | modify: v3 fields (Classic + Casual), `BORNPETS_CFG_KEYS` rows, `validate`, edge-table cache in `install` |
| `src/game/bornpets_cfg.rs` | modify: read `BPETS_V3.CFG` instead of `BORNPETS.CFG`; custom flag = values differ from preset; validation before install |
| `assets/to-badge/BPETS_V3.CFG` | create: v3 template, every key commented out with its Classic and Casual value |
| `scripts/copy_assets.py`, `scripts/test_copy_assets.py` | modify / create: `--update` mode that copies only files missing on the volume, `BPETS_V3.CFG` included; the old `BORNPETS.CFG` is not touched |
| `USER_GAMES.md` | modify: v3 key list (generated from `BORNPETS_CFG_KEYS`) |
| `src/game/engine/regime.rs` | create: `rates`, multipliers, `sleep_quality_pm`, `health_ceiling`, `mood_target` |
| `src/game/engine/solver.rs` | create: `seg_len`, `sig` |
| `src/game/engine/advance.rs` | create: `advance`, wake handling |
| `src/game/engine/doses.rs` | create: actions as doses |
| `src/game/engine/status.rs` | create: `StatusWord`, `status` |
| `src/game/engine/mod.rs` | modify: `GameState` fields, `update`, `next_wake_tick`, `PetStats`, save v3 |
| `src/game/engine/sim.rs` | modify: oracle driver, routines |
| `src/game/engine/to_display.rs` | modify: `display_anim` from `status` |
| `src/game/lifecycle.rs` | modify: `try_load` format-2 fallback and `pick_save`, save writer key `state_v3` (line 857), action wrappers |
| `src/game/modal.rs` | modify: Rest and Heal lists, refusal toasts |
| `tests/engine_v3.rs` | create |
| `tests/save_format.rs`, `tests/game_engine.rs` | modify |

---

### Task 1: needs, tunable bands and the edge table

**Files:** create `src/game/engine/needs.rs`; modify `src/game/engine/mod.rs` (add `pub mod needs;`), `src/game/engine/thresholds.rs`, `src/game/bornpets_cfg.rs` (`load_and_install` 27, `apply_overrides` 55); create `tests/engine_v3.rs`.

**Produces:**
```rust
// needs.rs
pub const P: i32 = 655; pub const MAX: i32 = 65535;
#[repr(u8)] pub enum Need { Full = 0, Water, Energy, Fun, Dirt, Booze, Strain, Fit, Weight }
pub const NEEDS: usize = 9;
pub fn full_lo() -> i32;  pub fn full_hi() -> i32;   // FULL_LO_PCT() as i32 * P, ...
pub fn water_lo() -> i32; pub fn water_hi() -> i32;
pub fn energy_lo() -> i32;
pub fn fun_lo() -> i32;   pub fn fun_hi() -> i32;
pub fn dirt_hi() -> i32;  pub fn booze_hi() -> i32;  pub fn booze_danger() -> i32;
pub fn strain_inj() -> i32; pub fn fit_lo() -> i32;
pub fn weight_lo() -> i32; pub fn weight_hi() -> i32;
pub const MAX_EDGES: usize = 8;
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EdgeTable { pub edges: [[u8; MAX_EDGES]; NEEDS], pub len: [u8; NEEDS] }
pub fn build_edges(t: &Thresholds) -> EdgeTable; // fixed edges merged with t's band edges, ascending, deduplicated
pub fn edges() -> &'static EdgeTable;            // cached copy built by thresholds::install
pub fn pct(level: i32) -> i32;  pub fn clamp_level(v: i32) -> i32;
// thresholds.rs
pub fn validate(t: &Thresholds, preset: &Thresholds) -> Thresholds; // per band pair: lo < hi else both from preset
```
Band fields (`u8`, percent; Classic / Casual): `FULL_LO_PCT` 38/33, `FULL_HI_PCT` 78/83, `WATER_LO_PCT` 38/33, `WATER_HI_PCT` 83/88, `ENERGY_LO_PCT` 33/28, `FUN_LO_PCT` 33/28, `FUN_HI_PCT` 73/78, `DIRT_HI_PCT` 58/63, `BOOZE_HI_PCT` 38/43, `BOOZE_DANGER_PCT` 70/75, `STRAIN_INJ_PCT` 60/65, `FIT_LO_PCT` 43/38, `WEIGHT_LO_PCT` 38/33, `WEIGHT_HI_PCT` 63/68. Setters clamp to 1..=99. Fixed edges (`const`, not tunable): Full 15, 25, 85, 95; Water 15, 25, 85, 90, 95; Energy 10, 50, 70, 85; Fun 40; Dirt 25, 85; Booze 10, 15, 30; Strain 15, 40; Fit 25, 50, 60, 61, 70; Weight 20, 65, 80. With Classic installed `edges()` equals the prototype's table: `[15,25,38,78,85,95]`, `[15,25,38,83,85,90,95]`, `[10,33,50,70,85]`, `[33,40,73]`, `[25,58,85]`, `[10,15,30,38,70]`, `[15,40,60]`, `[25,43,50,60,61,70]`, `[20,38,63,65,80]`.

All other v3 tunables (rates per minute, multipliers in per-mille, doses, timers in minutes, condition onset and cure times, ceiling steps) are added as `Thresholds` fields **in the task that first reads them** (Tasks 3, 4, 5, 9, 10), each with Classic = the value pinned in that task, Casual = Classic except health-damage rates halved, and one `BORNPETS_CFG_KEYS` row. Rates are stored as positive magnitudes (`u16`); the sign lives in the code. Any key used as a divisor is floored to 1 in its setter.

- [ ] Test `classic_edges_match_prototype`: `build_edges(&Thresholds::CLASSIC)` equals the nine slices above (compare `edges[i][..len[i]]`).
- [ ] Test `casual_bands_are_five_wider`: `Thresholds::CASUAL.FULL_LO_PCT == 33`, `FULL_HI_PCT == 83`, `FIT_LO_PCT == 38`; every slice of `build_edges(&CASUAL)` strictly ascending.
- [ ] Test `cfg_override_moves_an_edge`: `apply_overrides(&mut t, b"FULL_HI_PCT=80\n")` returns 1; `build_edges(&t).edges[Need::Full as usize]` contains 80, not 78.
- [ ] Test `bad_band_pair_falls_back_to_preset`: overrides `FULL_LO_PCT=90` with Classic: `validate` gives 38 / 78; `WATER_HI_PCT=0` clamps to 1 and then the pair falls back to 38 / 83.
- [ ] Test `custom_flag_only_when_values_differ`: a file whose rows repeat Classic values leaves `values == Thresholds::CLASSIC` and `load_and_install`'s custom flag is computed as `values != mode.preset()` (move the comparison into a host-testable `fn is_custom_set(values: &Thresholds, mode: Mode) -> bool`; derive `PartialEq` on `Thresholds`).
- [ ] Run `... --test engine_v3 classic_edges` : FAIL (module missing).
- [ ] Implement: fields, setters, `validate`, `build_edges`, cache filled in `thresholds::install` (also in the simulator's install path), `is_custom_set`; `load_and_install` calls `validate` before `install`.
- [ ] Run all five: PASS. Run the existing `tests/` suite: no regressions. Commit `feat(engine): v3 needs with tunable bands and edge table`.

### Task 2: GameState v3 fields and fresh egg

**Files:** modify `src/game/engine/mod.rs` (`GameState` 204, `new_egg`).

**Produces** (fields; old names are renamed, storage stays `u16` unless noted):
```rust
// renamed, now good-high levels: hunger -> fullness, tired -> energy, sick -> health, miserable -> mood,
// drunk -> booze; weight keeps its name and becomes body mass (50 % = ideal)
pub hydration: u16, pub fun: u16, pub strain: u16, pub dirt: u16, pub fitness: u16,
// minute timers
pub sleep_min: u16, pub oversleep_min: u16, pub good_sleep_min: u16, pub rested_min: u16,
pub injured_min: u16, pub groggy_min: u16, pub hangover_min: u16, pub rehab_min: u16,
pub recovering_min: u16, pub junk_min: u16, pub ozempic_min: u16, pub med_cd_min: u16,
pub insulin_min: u16, pub ozempic_cd_min: u16, pub reversal_min: u16,
// counters (existing u32 fields reused, now in minutes): drunk_ticks -> dependence_min, overweight_ticks -> diabetes_min,
// leaving_countdown -> leaving_min
pub booze_peak: bool, pub deprived: bool, pub night: bool, pub no_groggy: bool,
pub short_nights: u8, pub good_nights: u8, pub tick_rem: u8,
pub care_good_min: u32, pub care_awake_min: u32,
```
`alcoholic` is renamed `dependent`. Removed: all `cooldown_*` except `cooldown_battle`, `cooldown_onlypets` and the mini-game ones; `miserable_interval_counter`, `tired_passive_counter`, `cooldown_relax`.

Fresh egg: fullness 60 %, hydration 65 %, energy 80 %, fun 50 %, dirt 10 %, booze 0, strain 0, fitness 60 %, weight 50 %, mood 70 %, health `85 % - (MAX - vitality) / 8`.

- [ ] Test `new_egg_starts_inside_every_band`: after hatching, each of fullness, hydration, energy, fun, weight lies inside its band constants from Task 1; dirt < `DIRT_HI`; fitness >= `FIT_LO`.
- [ ] Run: FAIL (fields missing). Implement. Fix compile errors in `mod.rs` by stubbing removed rule code with `todo!()` only inside functions that later tasks replace (`update`, `consume_action_ticks`, `next_wake_tick`, `stats`, `to_bytes`, `from_bytes`); nothing outside `src/game/engine/` may reference a removed field after this task: replace such reads with the `PetStats` field of the same meaning.
- [ ] Run: PASS. Commit `feat(engine): GameState v3 fields`.

### Task 3: regime rates

**Files:** create `src/game/engine/regime.rs`.

**Consumes:** Task 1 constants, `GameState`. **Produces:**
```rust
pub fn rates(s: &GameState) -> [i32; NEEDS];      // per minute, order of `Need`
pub fn energy_cap(s: &GameState) -> i32;          // 70 * P when deprived, else MAX
pub fn sleep_quality_pm(s: &GameState) -> i32;    // 600 or 1000
pub fn thirst_pct(s: &GameState) -> i32;          // 100 / 110 / 125 / 140 by booze < 15 / < 40 / < 70 / else
```
Pinned values (per minute). Awake: fullness -54; water `-45 * thirst_pct / 100`; energy `-36 * m / 1000` with `m = 1000`, times 125/100 if fitness < `FIT_LO`, 90/100 if fitness >= 70 %, 150/100 if booze > `BOOZE_HI`, 150/100 if fun > `FUN_HI`, 125/100 if `hangover_min > 0`, 115/100 if weight outside its band; fun -40. Asleep: fullness -30; water `-20 * thirst_pct / 100`; energy `+120 * sleep_quality_pm / 1000` while energy < `energy_cap`, else 0; fun -40 while above 40 %, else 0. Always: dirt +8; booze -36, times 2/3 if hydration < `WATER_LO`, times 2/3 if dependent; strain -80; fitness -4 above 50 %, -2 above 25 %, else -1, times 3/2 if weight > `WEIGHT_HI`; weight `(-3 if > WEIGHT_HI, 0 if < WEIGHT_LO, else -1) + (-3 if fullness < FULL_LO, +3 if fullness > FULL_HI)`. Unsettled sleep (quality 600): booze > 30 %, fullness > 85 % or < 25 %, hydration < 25 % or > 90 %, fun > `FUN_HI`, or `rehab_min > 0`.

- [ ] Tests `awake_rates_in_band` (fresh egg awake: `[-54, -45, -36, -40, 8, -36, -80, -4, -1]`), `hung_over_sluggish_pet_drains_faster` (fitness 30 %, `hangover_min = 10`: energy rate `-36 * 1250 / 1000 * 125 / 100` computed with the same integer order = -56), `unsettled_sleep_recovers_at_600` (asleep, booze 35 %: energy rate 72), `deprived_pet_stops_recovering_at_70` (asleep, deprived, energy 70 %: 0).
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): regime rates`.

### Task 4: mood target and health terms

**Files:** modify `src/game/engine/regime.rs`.

**Produces:**
```rust
pub fn mood_target(s: &GameState) -> i32;
pub fn health_ceiling(s: &GameState) -> i32;   // P * (55 | 65 | 80 | 100 by fitness < 25 | < 50 | < 70 | else), minus 20 * P if diabetic && insulin_min == 0
pub fn health_damage(s: &GameState) -> i32;    // per minute
pub fn health_recovery(s: &GameState) -> i32;  // per minute, before the ceiling
pub const MOOD_STEP: i32 = 120;
```
`mood_target = MAX - sum`, floored at 0. Terms: `(FULL_LO - fullness) * 3 / 2`; `fullness - FULL_HI`; `(WATER_LO - water) * 3 / 2`; `water - WATER_HI`; awake only `(ENERGY_LO - energy) * 3 / 2` and `FUN_LO - fun`; `(fun - FUN_HI) * 3 / 2`; `booze - BOOZE_HI`; `dirt - DIRT_HI`; `(FIT_LO - fitness) / 2`; `(weight - WEIGHT_HI) / 2`; `(WEIGHT_LO - weight) / 2`; `50 * P - health`; each only when positive. Flat: injured 20 P, groggy 10 P, hangover 15 P, deprived 10 P, `rehab_min > 0` 20 P, else dependent and booze < 10 % and awake 15 P.
Damage: fullness < 15 % 40; > 95 % 8; water < 15 % 40; > 95 % 8; energy < 10 % awake and health > 30 % 8; booze > `BOOZE_DANGER` 40; mood < 20 % 10; dirt > 85 % 10; weight > 80 % 2; weight < 20 % 10.
Recovery: 14 if mood >= 60 %, 5 if >= 40 %, else 0; halved if deprived.

- [ ] Tests `target_is_full_when_all_in_band` (= `MAX`), `hungry_and_bored_penalties_add` (fullness 20 %, fun 10 %, awake: `MAX - (18 * P * 3 / 2) - 23 * P`), `ceiling_steps` (fitness 24 / 49 / 69 / 70 % give 55 / 65 / 80 / 100 %; diabetic without insulin at 70 %: 80 %), `sleep_shortage_stops_damaging_at_30` (energy 5 % awake, health 30 %: damage 0).
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): mood target and health terms`.

### Task 5: closed-form advance

**Files:** create `src/game/engine/advance.rs`.

**Produces:**
```rust
/// Advance by `dt` minutes at the rates of the regime at the start. Caller guarantees no threshold is crossed (Task 7).
pub fn advance(s: &mut GameState, dt: u32);
pub fn wake(s: &mut GameState, by_player: bool);
```
Order inside `advance` (identical to `simv3` `advance`): sleep counters; slow counters by start regime (`dependence_min += dt` while booze > `BOOZE_HI`, else minus `dt / 4` while booze < 15 %; same pair for `diabetes_min` with weight > 80 % / < 65 %; `reversal_min += dt` while diabetic and weight < 65 % and fitness > 60 %); the nine needs by `rates * dt`, clamped, energy capped at `energy_cap` while asleep, fun not below 40 % while asleep; booze peak and hangover (peak when booze > `BOOZE_HI`; when peaked and booze < 15 %: `hangover_min = 480`); flags (dependent at 1200, or 400 while `recovering_min > 0`; diabetic at 4320; reversal at 10080 clears diabetic); timers minus `dt` (hangover minus `2 * dt` asleep); rehab end (clears dependent, `dependence_min = 0`, `recovering_min = 10080`); self-wake at `sleep_min >= 720` or `oversleep_min >= 300`; collapse at energy 0 (asleep, `night = true`, health -3 P only while health > 33 %); mood `clamp(target, mood - 120 * dt, mood + 120 * dt)`; health `h = health - damage * dt; if h < ceiling { min(h + recovery * dt, max(ceiling, h)) } else { max(h - 3 * dt, ceiling) }`; leaving (`leaving_min += dt` at health 0, Gone at 720; otherwise minus `dt / 2`); care counters.
`wake`: grumpy (`by_player` and energy < 50 %: mood -10 P); night bookkeeping (`night` and `sleep_min < 360`: `short_nights += 1`, `good_nights = 0`, deprived at 3; `night` and `sleep_min >= 360`: `short_nights = 0`, and at `>= 390` `good_nights += 1`, clears deprived at 2); `no_groggy` clears `oversleep_min` once; groggy when `oversleep_min > 240` (`groggy_min = 120`, energy -10 P, fitness -2 P); well rested when `good_sleep_min >= 300` (`rested_min = 960`).

- [ ] Tests: `one_minute_awake_moves_needs_by_their_rates`; `sleep_from_30_percent_is_full_in_about_six_and_a_half_hours` (advance in 1-minute calls: energy reaches `MAX` between minute 375 and 390); `oversleep_past_four_hours_makes_groggy` (asleep at full energy 250 min, `wake(true)`: `groggy_min == 120`); `three_short_nights_deprive_two_good_nights_clear`; `hangover_starts_after_peak_and_lasts_480`; `rehab_course_cures_and_marks_recovering`; `diabetes_reverses_after_10080_lean_fit_minutes`.
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): closed-form advance and wake`.

### Task 6: update() in minutes with tick remainder

**Files:** modify `src/game/engine/mod.rs` (`update` 540). Hatching, hibernation and Gone handling stay as they are.

**Produces:** `pub fn update(&mut self, now_tick: u32)` with: `elapsed = now_tick - last_update_tick + tick_rem`; `minutes = elapsed / 6`; `tick_rem = (elapsed % 6) as u8`; loop `while minutes > 0 { let dt = solver::seg_len(self, minutes); advance(self, dt); minutes -= dt; }`. Until Task 7 lands, `seg_len` returns 1.

- [ ] Test `many_small_updates_equal_one_big_update`: two eggs past hatching; A gets `update(t)` for every tick `t` in 1..=8640; B gets one `update(8640)`; assert `A.fullness == B.fullness`, `A.energy == B.energy`, `A.age_ticks == B.age_ticks`, and `B.fullness < 60 * 655` (it aged).
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): update in whole minutes with carried remainder`.

### Task 7: table-driven solver

**Files:** create `src/game/engine/solver.rs`.

**Produces:**
```rust
/// Largest dt in 1..=max (minutes) for which no need reaches a threshold and no timer expires.
pub fn seg_len(s: &GameState, max: u32) -> u32;
```
Per need with rate `r != 0` and not pinned at 0 or `MAX` against its direction: for every threshold `T` (table entries times `P`, plus 0 and `MAX`) ahead in the direction of travel, `n = (|T - level| - 1) / |r|`; take the minimum. Timers: every non-zero minute timer; `720 - sleep_min` asleep; `300 - oversleep_min` and `241 - oversleep_min` while asleep at the energy cap; `720 - leaving_min` at health 0; `threshold - dependence_min` while booze > `BOOZE_HI` and not dependent; `4320 - diabetes_min` while weight > 80 % and not diabetic; `10080 - reversal_min` while reversing; hangover as `(hangover_min + 1) / 2` asleep. Result at least 1.

- [ ] Tests: `segment_stops_one_unit_before_a_threshold` (fullness 40 %, awake: `seg_len(s, 10_000) == (40 * 655 - 38 * 655 - 1) / 54`); `timer_bounds_the_segment` (`injured_min = 7`: 7); `pinned_level_is_ignored` (asleep at energy `MAX`, only timers bound it); `never_zero`.
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): table-driven segment solver`.

### Task 8: mood and health zone check

**Files:** modify `src/game/engine/solver.rs`, `src/game/engine/mod.rs` (`update`).

**Produces:**
```rust
pub fn sig(s: &GameState) -> u32;   // packs: mood zone (>=80, >=60, >=40, >=20, else), health zone (0, <25, <30, <45, <50, else), asleep, gone, dependent, diabetic, hangover, deprived, sign(health - ceiling)
```
In `update`: after `dt = seg_len(..)`, while `dt > 1`: advance a copy by `dt`; if `sig(copy) != sig(self)` set `dt = (dt + 1) / 2` and retry; then advance `self`.

- [ ] Test `mood_crossing_60_splits_the_segment`: pet with all needs in band except fun at 0 held by rate (asleep is not used; set fun 0 %, mood 90 %); one `update` of 600 minutes versus 600 `advance(1)` calls on a clone: health differs by at most `655`, mood identical.
- [ ] Run: FAIL (health off by more than one percent). Implement. Run: PASS. Commit `feat(engine): split segments at mood and health lines`.

### Task 9: doses

**Files:** create `src/game/engine/doses.rs`; modify `src/game/engine/mod.rs` (`start_action` 1170, `consume_action_ticks` 832, action starters, `Action` enum gains `Wash`, `Relax`), `src/game/engine/food.rs`, `src/game/engine/drink.rs`.

**Produces:**
```rust
pub enum Refusal { NotThirsty, AlreadyClean, Injured, Asleep, NoNeed }
pub fn apply(s: &mut GameState, a: Action) -> Result<(), Refusal>;   // called when the action animation ends
pub fn wash(&mut self) -> bool; pub fn relax(&mut self) -> bool;     // on GameState, same shape as feed()/play()
```
Starters keep the existing gate (alive, no action running, price) but no per-action cooldown; a care action on a sleeping pet calls `advance::wake(s, true)` first. Refusals are checked before waking and before charging.
Pinned doses (percent points unless noted; from spec 2.3, 15.2 to 15.5, 17, 18):

| Action | Effect |
|---|---|
| Salad / Apple | fullness +20 / +25; hydration +8; dirt +10; Salad fitness +1; Apple weight +0.5 |
| Frikandel / Pizza / Cake | fullness +35 / +40 / +10; weight +2 / +2 / +3; dirt +10 / +10 / +5; mood +6 / +8 / +15 if `junk_min == 0` else a third, none while `ozempic_min > 0`; then `junk_min = 180` |
| any food | excess = fullness after the meal above `FULL_HI` (counting up to 120 %): energy minus excess / 2, weight plus excess / 4 |
| Water | refused above 85 % hydration; hydration plus `min(30, 90 - hydration)`; fullness +2 only if at least 15 was taken; booze -5; `hangover_min` -60; `groggy_min` -30 |
| Cola | hydration +20; fullness +2; energy +8; mood +3; weight +1 |
| Beer / Wine / Whiskey | hydration +20 / +8 / 0; fullness +2 / +1 / 0; booze +15 / +20 / +30, times 3/2 if energy < `ENERGY_LO`; mood +12 / +15 / +20 and energy +5, both times `(100 - booze %) / 100`, halved if dependent; fun +5; Beer weight +1; during `rehab_min > 0`: `rehab_min = 0`, mood -20 |
| Play | refused if injured; fun +25 (doubled if energy < `ENERGY_LO`); energy -5; fullness -3; hydration -4; fitness +2; dirt +3; `groggy_min` -60 |
| Mini-game grade g | fun `10 + 7 g` (doubled when tired); energy -3 |
| Exercise | refused if injured; strain +35, times 3/2 if fullness < `FULL_LO` or energy < `ENERGY_LO` or hydration < `WATER_LO`, times 7/10 if `rested_min > 0`, times 85/100 if fitness >= 70 %, times 6/5 if weight > `WEIGHT_HI`, times 5/4 if deprived; energy -10; fullness -8; hydration -12; fun +10; booze -5; dirt +12; weight -3; `hangover_min` -120; `groggy_min = 0`; if strain > `STRAIN_INJ`: `injured_min = 1440`, health -5; else fitness +12 |
| Wash | refused below 25 % dirt; dirt -70 |
| Relax | fun -25; strain -15; energy +3; fitness -1 |
| Heal (medicine) | 240-minute cooldown `med_cd_min`; injured: `injured_min` -720 and booze +10; else health > 80 %: booze +10; else health +10 |
| Medicate (insulin) | only if diabetic and `insulin_min <= 120`; `insulin_min = 480` |
| Ozempic | `ozempic_cd_min == 0`; weight -15; fitness -5; `ozempic_min = 1440`; `ozempic_cd_min = 2880` |
| Rehab | only if dependent and `rehab_min == 0`; `rehab_min = 4320` |
| Sleep | `night = energy < 50 %`; asleep; `sleep_min = 0`; `good_sleep_min = 0` |
| Wake | `advance::wake(s, true)` |
| Only pets (work) | refused if energy < `ENERGY_LO` or injured; pay as today's `ONLYPETS_HOBBY_REWARD` until plan 6; energy -15; fun +10; mood -5 |
| friend_boost(big) | fun +15 / +5 instead of mood |

- [ ] Tests (one per row that has a rule beyond addition): `overeating_becomes_food_coma_and_weight`; `water_refused_above_85_and_never_past_90`; `second_treat_within_three_hours_gives_a_third`; `beer_on_a_tired_pet_adds_22_booze` (15 * 3 / 2); `beer_during_rehab_is_a_relapse`; `sport_while_hungry_injures_on_the_second_session`; `wash_refused_when_clean_and_does_not_wake`; `medicine_on_an_injury_halves_it_and_adds_booze`; `care_action_wakes_a_sleeping_pet_grumpy_under_50`.
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): actions as doses`.

### Task 10: next_wake_tick, status and the 2 h cap

**Files:** create `src/game/engine/status.rs`; modify `src/game/engine/mod.rs` (`next_wake_tick` 1436, `PetStats` 1571, `stats` 1717), `src/game/engine/thresholds.rs` (`MAX_SLEEP_TICKS: 720` in both presets), `src/game/engine/to_display.rs` (91).

**Produces:**
```rust
#[repr(u8)] pub enum StatusWord { Content, Hungry, Stuffed, Thirsty, Waterlogged, Tired, Bored, Wired, Sloppy, Grubby,
    Overweight, Underweight, Unfit, HungOver, Injured, Groggy, Poorly }
pub fn status(s: &GameState) -> (StatusWord, bool /* danger */);   // largest mood penalty term; Content under 3 * P
pub fn next_wake_tick(&self) -> u32;
```
`next_wake_tick`: Gone or hibernating: `u32::MAX`; hatching: as today; otherwise copy the state and step it with `seg_len` + `advance` until `status`, asleep, phase or the action changes, or 120 minutes are used; return `last_update_tick + minutes * 6 - tick_rem`, capped at `last_update_tick + MAX_SLEEP_TICKS()`; an action end is still its own bound.
`PetStats` gains `water, fun, clean, booze, weight_pct, fit, status, danger, asleep_min` and keeps `hunger, tired, healthy, happy, weight, drunk` as display percentages of fullness, energy, health, mood, fitness, `100 - booze` so today's screens compile unchanged. `can_*` flags lose their cooldown terms.
`display_anim`: keep the priority order; map `Hungry/Thirsty` to Warning or Critical hungry by `danger`, `Tired/Groggy/HungOver` to Warning or Critical tired, `Poorly/Injured` to Warning or Critical sick, `Bored/Wired/Sloppy/Grubby/Stuffed/Waterlogged/Overweight/Underweight/Unfit` to Warning miserable, `Content` to Happy when mood >= 80 % else Idle.

- [ ] Tests: `wake_is_never_later_than_first_status_change` (for a fresh pet and for a pet at fullness 40 %: step a clone minute by minute until `status` changes at minute `m`; assert `next_wake_tick() <= last_update_tick + m * 6`); `wake_is_capped_at_720_ticks`; `wake_budget_balanced_day` (run the balanced routine of Task 12 for 30 days sleeping until `next_wake_tick` between player events; engine wake-ups per day `<= 20`); `status_picks_the_largest_penalty`.
- [ ] Run: FAIL. Implement. Run: PASS. Commit `feat(engine): status word, exact wake scheduling, 2 h cap`.

### Task 11: save v3 and the v2 upgrade

**Files:** modify `src/game/engine/mod.rs` (1902 to 2110), `src/game/lifecycle.rs` (`try_load` 113, which reads the keys; the save writer `ns.set("state", ...)` at line 857 in `save_if_needed` becomes `ns.set("state_v3", ...)`), `tests/save_format.rs`.

**Produces:**
```rust
pub const SAVE_FORMAT_VERSION: u8 = 3;
pub const SAVE_SIZE: usize = 177;
pub const SAVE_SIZE_V2: usize = 97;
pub fn from_bytes(b: &[u8]) -> Option<Self>;            // v3 only
pub fn from_bytes_v2(b: &[u8; SAVE_SIZE_V2]) -> Option<Self>;
/// Pure and host-testable: choose the state to load from the raw kv values.
/// `v3` is the value of key "state_v3", `v2` the value of the old key "state".
/// The bool is true when the state came from the format-2 upgrade and must be saved at once.
pub fn pick_save(v3: Option<&[u8]>, v2: Option<&[u8]>) -> Option<(GameState, bool /* upgraded */)>;
```
v3 layout: bytes 0..97 keep the v2 offsets and widths, with the renamed meanings of Task 2 (`cooldown_*` slots that no longer exist are written as 0, except `cooldown_medicate` = `insulin_min`, `cooldown_ozempic` = `ozempic_cd_min`, `cooldown_heal` = `med_cd_min`); then, little-endian: hydration, fun, strain, dirt, fitness (5 x u16); sleep_min, oversleep_min, good_sleep_min, rested_min, injured_min, groggy_min, hangover_min, rehab_min, recovering_min, junk_min, ozempic_min, reversal_min (12 x u16); flags u8 (bit 0 booze_peak, 1 deprived, 2 night, 3 no_groggy); short_nights u8; good_nights u8; tick_rem u8; care_good_min u32; care_awake_min u32; 34 reserved zero bytes.
`from_bytes_v2`: requires version byte 2. fullness = `clamp(MAX - hunger, 25 %, 78 %)`; energy = `MAX - tired`; health = `MAX - sick`; mood = `MAX - miserable`; booze = drunk; weight = `50 % + old_weight / 2`; fitness 60 %; hydration 60 %; fun 50 %; dirt 20 %; strain 0; `insulin_min = cooldown_medicate / 6` capped at 480; `dependence_min = drunk_ticks / 6` capped at 1199; `diabetes_min = overweight_ticks / 6` capped at 4319; flags `diabetic`, `alcoholic -> dependent`, phase, traits, age, money, generation, wins, losses, pet kind, hibernation copied; a running action is dropped; `is_sleeping` kept with `night = true`.
Save keys (owner: so that the old firmware can be restored): the v3 save is written under kv namespace "game", key `state_v3`. The old key `state` is read only for the format-2 upgrade and is never written or deleted, so a player who flashes the old firmware again finds their old pet. `try_load` reads `state_v3` first (177 bytes go to `from_bytes`); otherwise it reads `state` (97 bytes go to `from_bytes_v2`) and marks the state for an immediate save, which goes to `state_v3`. When both exist, `state_v3` wins. `try_load` only fetches the two values and calls `pick_save`; a `state_v3` that is present but corrupt falls through to `state`.

- [ ] Tests: `pick_save_prefers_v3` (both keys valid: the v3 state wins, `upgraded == false`); `pick_save_upgrades_v2_when_no_v3` (only the 97-byte `GAME_GOLDEN`: `Some`, `upgraded == true`); `pick_save_none_when_both_bad` (wrong lengths or versions in both: `None`); `v3_golden_bytes_round_trip` (new 177-byte literal captured from the encoder once it exists; assert byte 0 == 3 and every new field round-trips); `v2_golden_upgrades_to_expected_v3_levels` (decode the existing `GAME_GOLDEN` 97-byte literal with `from_bytes_v2`: result is `Some`, phase unchanged, fullness within 25..=78 %, `fitness == 60 * 655`, `weight >= 50 * 655`, `health == 65535 - old sick`, money and generation equal to the v2 values); `v3_rejects_v2_length_and_v2_decoder_rejects_v3`; update `game_state_rejects_wrong_version_and_length` for 177 and 3.
- [ ] Run: FAIL. Implement. Run: PASS. Also run the firmware check command: builds. Commit `feat(engine): save format v3 with v2 upgrade`.

### Task 12: simulator oracle and routines

**Files:** modify `src/game/engine/sim.rs`, `src/bin/simulate_game.rs`, `tests/game_engine.rs` (replace the v1 rule tests; keep hatching, reproducibility and seed tests), `tests/engine_v3.rs`.

**Produces** (simulator feature only):
```rust
pub enum Driver { Oracle, Delta }
pub struct Outcome { pub alive: bool, pub days: f32, pub avg_mood_pct: u8, pub end_health_pct: u8, pub min_health_pct: u8,
    pub end_fitness_pct: u8, pub end_weight_pct: u8, pub injuries: u16, pub engine_wakes_per_day: f32 }
pub fn run_routine(routine: &dyn Fn(u32, u32, &GameState) -> heapless::Vec<Action, 6>, days: u32, driver: Driver) -> Outcome;
pub fn balanced(day: u32, minute: u32, s: &GameState) -> heapless::Vec<Action, 6>;   // and sporty, lazy_treats, party, short_sleeper, spam_water, spam_wash, once_a_day, absent
```
`Driver::Oracle` calls `advance(1)` for every minute; `Driver::Delta` calls `update()` only at routine events and at each day end. Routines are ported from `simv3` (`balanced`, `sporty`, `lazy_treats`, `party`, `short_sleeper`, `spam_p`, `once_a_day`, `absent`) including the water habit (water at a check-in under 55 % hydration, two under 25 %) and the weight habits of spec 15.8.

- [ ] Tests in `tests/engine_v3.rs`:
  - `delta_matches_oracle_all_routines`: for each routine, 30 days: `alive` equal; if both alive `|avg_mood| <= 1`, `|end_health| <= 2`, `|end_fitness| <= 2`, injuries equal. The party routine is exempt from the health bound (spec 18.3 notes the divergence).
  - `fourteen_day_gap_matches_oracle`: fresh pet asleep; one `update` across 14 days versus the oracle: phase equal, `|health| <= 2 %`.
  - `design_targets`: balanced alive with `avg_mood_pct >= 90`; sporty and lazy alive; lazy `end_health_pct` in 55..=70; short sleeper alive with `min_health_pct >= 30`; spam water and spam wash alive; once a day and absent not alive, absent gone before day 3.
- [ ] Run: FAIL. Implement. Run: PASS (expect about a minute on two cores). Commit `feat(sim): step oracle, routines and v3 design targets`.

### Task 13: firmware wiring

**Files:** modify `src/game/lifecycle.rs` (wrappers `wash`, `relax`, `wake` already exists at 588), `src/game/modal.rs` (Rest list 176: `Sleep, Relax, WakeUp-from-sleep, Cancel`; a `Wash` item under Rest; `Water` refusal and `Wash` refusal toasts `Not thirsty.` / `Already clean.`; remove cooldown suffixes for items without a cooldown), `src/game/mod.rs` (two `Toast` variants).

**Consumes:** `GameState::wash`, `relax`, `wake`, `Refusal`.

- [ ] Test (host, in `src/game/modal.rs` tests): `rest_menu_offers_sleep_relax_wash_wake` (the Rest item list contains the four items; `Wake` is available only while `stats.is_sleeping`, `Sleep` only while not).
- [ ] Run: FAIL. Implement. Run: PASS. Run the firmware check command: builds. Run `make sim` once and play one day by hand (feed, drink, wash, sleep, wake).
- [ ] Commit `feat(game): menu items for the v3 care actions`.

### Task 14: boot catch-up of at most 12 h

**Files:** modify `src/game/lifecycle.rs` (`try_load` 113, `init` 67).

**Produces:** `pub fn catch_up_minutes(saved_unix: u32, now_unix: u32) -> u32` returning `min((now - saved) / 60, 720)`, 0 if either time is 0 or `now < saved`; a `saved_unix: u32` field written by `save_if_needed` into 4 of the reserved bytes of Task 11 (offset 143). On load with a known clock, call `update` with the equivalent ticks.

- [ ] Test `catch_up_is_capped_and_safe`: `(0, x) -> 0`; `(1000, 900) -> 0`; `(1000, 1000 + 3600) -> 60`; `(1000, 1000 + 86400 * 3) -> 720`.
- [ ] Run: FAIL. Implement. Run: PASS. Firmware check builds. Commit `feat(game): catch up at most 12 h on boot`.

### Task 15: v3 config template, update copy, docs

**Files:** modify `src/game/bornpets_cfg.rs` (read `BPETS_V3.CFG` instead of `BORNPETS.CFG`); create `assets/to-badge/BPETS_V3.CFG`; modify `scripts/copy_assets.py` (`copy_assets_to` 140, `process_device` 170, `main` 395); create `scripts/test_copy_assets.py`; modify `USER_GAMES.md` (section "Custom balance", line 46, already retitled `BPETS_V3.CFG`), `QUICKSTART.md:93`, `POCKET_CARD.md:62`.

**Consumes:** `BORNPETS_CFG_KEYS`, `Thresholds::CLASSIC`, `Thresholds::CASUAL`, `apply_overrides`. **Produces:**
```rust
// thresholds.rs, host only (#[cfg(any(test, feature = "simulator"))])
pub fn cfg_template() -> String; // the exact text of assets/to-badge/BPETS_V3.CFG
```
```python
# copy_assets.py
CONFIG_NAME = "BPETS_V3.CFG"
def update_assets_to(mount_point: Path, assets_dir: Path) -> dict  # {"added": [names]}
```
Owner decision (2026-11-13): the v3 firmware does not rename the current config. It reads a new file, `BPETS_V3.CFG`, so players can go back to the old firmware, which keeps using its own `BORNPETS.CFG`. The old `BORNPETS.CFG` is never read, written, renamed or deleted by v3 or by the update script. There is no `.BAK` and no backup logic. `BPETS_V3` is 8 characters, so the name is 8.3-safe for the badge's FAT12 volume.

Template text: first line `# BornPets v3 balance. Uncomment a line to change it. The old firmware keeps using BORNPETS.CFG.`; then for every key in `BORNPETS_CFG_KEYS` order one comment line `# KEY=<classic>   (Casual <casual>)`. No active rows, so the file never overrides the Casual preset and never sets the `*` mark.

Update mode (`copy_assets.py --update`): accepts non-empty `CYBR*` volumes and copies only the asset files that are **missing** on the volume. That includes `BPETS_V3.CFG`. An existing `BPETS_V3.CFG` is never overwritten, because the owner may have edited it. Existing files are never overwritten at all (hand-added PCX, `PETS.CFG`, `000000.PCX`, `010000.PCX` differ per badge), and `BORNPETS.CFG` is left alone. Factory mode (no flag) is unchanged.

- [ ] Test `template_parses_to_classic_with_no_edits` (Rust, `tests/engine_v3.rs`): `cfg_template()` equals the file on disk (`include_str!("../assets/to-badge/BPETS_V3.CFG")`); `apply_overrides(&mut CLASSIC.clone(), template)` returns 0; every key of `BORNPETS_CFG_KEYS` appears once; uncommenting every line yields `values == CLASSIC`.
- [ ] Test `no_v3_key_reuses_a_v1_name`: none of the v3 keys equals one of the 21 v1 names (`HUNGER_RATE`, `TIRED_RATE`, `SLEEP_HUNGER_COST`, `SICK_RATE`, `SICK_CONDITION_RATE`, `SICK_CONDITION_MISERABLE_RATE`, `MISERABLE_INTERVAL_BASE`, `MISERABLE_INTERVAL_MIN`, `FEED_HUNGER_RELIEF`, `HEAL_SICK_RELIEF`, `PLAY_HUNGER_COST`, `PLAY_TIRED_COST`, `MINIGAME_HUNGER_COST`, `MINIGAME_COOLDOWN`, `HATCHING_TICKS`, `MAX_SLEEP_TICKS`, `WEIGHT_RATE`, `FEED_WEIGHT_GAIN`, `EXERCISE_WEIGHT_RELIEF`, `DIABETES_ONSET_TICKS`, `MEDICATE_COOLDOWN`) unless the field keeps its v1 meaning and unit (`HATCHING_TICKS`, `MINIGAME_COOLDOWN`, `MAX_SLEEP_TICKS`); v1 rows whose field the v3 engine no longer reads are removed from `BORNPETS_CFG_KEYS`.
- [ ] Tests (Python, `scripts/test_copy_assets.py`, temp dirs): `test_update_leaves_old_config_untouched` (an existing `BORNPETS.CFG` is byte-identical afterwards and no `.BAK` appears); `test_update_adds_v3_config_when_missing` (no `BPETS_V3.CFG` on the volume: it is copied from the assets); `test_update_keeps_edited_v3_config` (an edited `BPETS_V3.CFG` survives byte-identical); `test_update_never_overwrites_other_files` (a differing `PETS.CFG` and `000000.PCX` survive, a missing PCX is added); `test_factory_mode_unchanged` (empty volume gets every file).
- [ ] Run `CARGO_BUILD_JOBS=2 taskset -c 0,1 cargo test -j 2 --features simulator --test engine_v3 template_` and `python3 -m unittest discover -s scripts -p 'test_copy_assets.py'`: FAIL.
- [ ] Implement `cfg_template`, write the asset file from it, switch `bornpets_cfg.rs` to `BPETS_V3.CFG`, `update_assets_to`, the `--update` flag; regenerate the key list in `USER_GAMES.md` from `BORNPETS_CFG_KEYS` (the stale v1 keys `DRAINED_INTERVAL`, `FEED_DRAINED_RELIEF`, `RELAX_DRAINED_RELIEF`, `RELAX_HUNGER_COST`, `PLAY_DRAINED_COST`, `DRAINED_INTERVAL_MISERABLE` go) and say that the v3 firmware reads `BPETS_V3.CFG` and leaves the old `BORNPETS.CFG` for the old firmware.
- [ ] Run both: PASS. Firmware check builds. Commit `feat(game): v3 BPETS_V3.CFG template, old config left for the old firmware`.

### Task 16: v3 debug menu (Konami code)

Owner (2026-11-13): "Add a stage to update the test menu that is opened when the Konami code is used." The hidden menu (Up Up Down Down Left Right Left Right Fire on the idle game screen, `src/game/debug_cheats.rs`, `Page::Debug` in `src/game/modal.rs:116`) still carries v1 cheats. After this task every v3 state can be reached in seconds on a badge, and the menu shows what the engine is doing.

**Files:** modify `src/game/engine/mod.rs` (`debug_*` block ~836), `src/game/lifecycle.rs` (debug wrappers ~553), `src/game/modal.rs` (`Page::Debug` list, `Item` enum, labels, dispatch); test `tests/engine_v3.rs`.

**Produces:**
```rust
pub enum DebugPoke { Hungry, Thirsty, Stuffed, Tired, Bored, Dirty, Fat, Thin, Unfit, Drunk, HungOver, Groggy, Injured,
    Diabetic, Dependent, Comfort /* every need mid-band, mood and health 70 % */, ClearConditions, ClearCooldowns }
impl GameState {
    pub fn debug_poke(&mut self, p: DebugPoke);   // one entry point; replaces the v1 debug_force_* / debug_clear_* methods
    pub fn debug_skip_minutes(&mut self, m: u32); // through update(), like real time
    pub fn debug_info(&self) -> DebugInfo;        // status word + danger, mood vs mood target, health, minutes to next wake, asleep/night
}
```
- Every poke sets the state through the same fields the rules read, so the next `update` behaves as if the pet got there naturally. A poke never starts or ends an action, never charges HAX, and sets `save_pending`.
- `Hungry`, `Thirsty`, `Tired`, `Bored` and `Dirty` put the need just past its danger line (`needs::*` consts, Task 10). `Stuffed`, `Fat`, `Thin` and `Unfit` put it just past the comfort band edge. `Drunk` sets booze just past `booze_danger()`. `HungOver`, `Groggy` and `Injured` set their timers to the dose values of Task 9 (`hangover_min`, `groggy_min`, `injured_min = 1440`). `Diabetic` and `Dependent` set the flags. `ClearConditions` clears the injury, the hangover, grogginess, dependence (with `dependence_min`), diabetes (with `diabetes_min`), rehab and reversal. `ClearCooldowns` zeroes `med_cd_min`, `ozempic_cd_min` and `insulin_min`, plus the mini-game and battle cooldowns.
- Menu rows (`Page::Debug`), in this order: Engine info, Skip 10 min, Skip 1 hour, Skip 1 day, Comfort, Hungry, Thirsty, Stuffed, Tired, Bored, Dirty, Fat, Thin, Unfit, Drunk, Hung over, Groggy, Injured, Diabetic, Dependent, Clear conditions, Clear cooldowns, Reset battle record. The items keep R24: a poke that cannot apply (pet hatching, gone or hibernating) shows its reason.
- "Engine info" shows `debug_info()` as a reason-style toast or text line (status word, `!` when in danger, mood / target, health, wake in Xm). This is how a tester checks the wake scheduler on hardware.
- One table maps menu row → label → `DebugPoke` / skip minutes (DRY), and the v1 `debug_force_*` / `debug_clear_*` methods and wrappers are removed (R15). The v1 labels "Trigger alcoholism" / "Clear alcoholism" go; v3 says "Dependent".

- [ ] Tests (`tests/engine_v3.rs`, `mod debug_menu`): `each_poke_shows_its_status_word` (from `Comfort`, every need poke makes `status()` name the matching word, with danger set for the danger pokes); `pokes_never_start_actions_or_charge`; `comfort_is_content`; `skip_minutes_equals_update` (`debug_skip_minutes(m)` equals `update(last + m * 6)` on the full state); `clear_conditions_and_cooldowns` (the flags, timers and cooldowns are 0 and `refusal_for` lets Heal, Ozempic and Medicate through when their precondition holds); `debug_info_matches_engine` (status, target and wake equal `status()`, `mood_target` and `next_wake_tick`).
- [ ] Run: FAIL. Implement. Run: PASS, plus `--lib` (the modal row table) and the firmware check build. Commit `feat(game): v3 debug menu`.

## Self-review

- **Coverage:** needs and bands (T1), fields (T2), every rate and multiplier of spec 13.2 / 15 (T3), mood and health incl. stepwise ceiling and the 30 % sleep-shortage floor (T4), sleep without timer, oversleep 4 h, sleep-deprived, hangover, dependence and rehab, diabetes and reversal, injury, collapse, leaving drain-back (T5), delta update (T6 to T8), every dose and refusal of 2.3 / 15 / 17 (T9), wake cap and status (T10), save (T11), oracle and targets (T12), minimal UI (T13), row 30 (T14). Tunable balance: bands and edge table (T1), every other v3 value added as a `Thresholds` field by the task that reads it, config template `BPETS_V3.CFG`, update copy and docs (T15). Deliberately outside this plan: demo policy, stations, battle formula, hard mode, the two-page stats view.
- **Type consistency:** levels are `u16` in the struct and `i32` in arithmetic; minute timers `u16` (largest value 10080); counters reuse the existing `u32` fields; `Action` gains `Wash` and `Relax` and needs matching `to_u8` / `from_u8` values 10 and 11.
- **Review Focus:** items 1 to 6 are pinned in T6, T12, T8, T11, T10, T1 + T15.
- **Proportion:** rule bodies are given as pinned values; only the solver and the segment split are described as algorithms.
