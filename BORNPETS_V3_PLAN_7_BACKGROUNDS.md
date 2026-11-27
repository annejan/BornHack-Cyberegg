# BornPets v3, plan 7: backgrounds, floor items, Jellyfish removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the pet lives somewhere. Behind the pet the game screen shows a day sky with a small sun, or a dithered night sky with a moon and stars, following the wall clock (night 21:00 to 07:00). Dirt shows as things on the floor: crumbs, a banana peel, a can, a sock, a pizza box, one more at each dirt line, all gone after a wash. A content pet gets a row of grass tufts, and a poorly pet a small red cross. The Jellyfish pet leaves the game. Its 50 sprite files free 75 FAT12 clusters. A badge whose live pet is a Jellyfish moves that pet to the Unicorn Realm with its record and shows "Moved to the / Unicorn Realm" above "Press Fire / for a new egg" (owner, 2026-11-14). `copy_assets.py --update` learns to remove and replace shipped files without touching per-badge files.

> **Pre-existing defect, fixed by this plan.** At `5e59574`, `assets/to-badge/` needs **505 of the 502 clusters** of the badge's FAT12 volume. A fresh full copy (`copy_assets.py`) runs out of space on its last file, `PETS.CFG`: the badge then has no roster and no pet can be picked. **Existing badges can be hit too.** A badge that already holds every current file needs the same 505 clusters, so today's `--update` fails with "no space" on the last missing file (often `BPETS_V3.CFG`, 6 clusters) and leaves a partial file behind. A badge provisioned with fewer files may still fit. Removing the Jellyfish (Task 7) brings the set to 429 clusters. Task 5 makes `--update` free space first (removals before additions) and delete a partial file when a copy runs out of space.

**Architecture:** what the background shows is a pure function of the pet, `engine::scene::background_state(&GameState) -> Option<BackgroundState>`. It reads the engine's local minute of the day (`day_min`, plan 3), dirt, the status line and health. Its packed byte is the 8th element of the engine's `Visible` tuple. The five floor lines are dirt lines in `solver::extra_levels`, and the next sky switch is a plain timer in `solver::timer_bound`. So `next_wake_tick` wakes exactly when the background changes, with no polling and no step loop. Drawing lives in a new UI module, `src/game/background.rs`, in `embedded-graphics` primitives and small const bitmaps. It is drawn into the framebuffer right before the sprite blit, in the firmware `render()` and in the simulator's blit block. It uses no image files and adds no refresh: it rides the frames the pet screen already draws. The Jellyfish goes through three changes. Assets are removed through a hash-guarded `assets/asset-changes.txt` that `--update` applies. The firmware hides pets that have no sprites (`pet_registry::drop_without_sprites`). At boot, a live pet of a hidden kind is retired through the existing Unicorn Realm path (`lifecycle::retire_hidden_pet`). Its record is pushed the way `new_generation` pushes a "Reset Pet", the pet becomes Gone, and the usual Gone screen leads to a new egg.

**Tech stack:** Rust, `no_std` firmware (embassy, embedded-graphics, FONT_7X13 / FONT_7X13_BOLD only), host tests through the `simulator` feature, Python 3 stdlib for `scripts/copy_assets.py`, Typst guide (`make guide`).

**Spec:** `BORNPETS_V3_DESIGN.md` §14 (14.1 to 14.5). Also §8.3 (the calm display items parked to this plan), §2.2 and §17 row 1 (dirt and the floor items), and §13.4 / §18.6 (wake and refresh counts). Roadmap row 7: `BORNPETS_V3_ROADMAP.md`. Format exemplar: `BORNPETS_V3_PLAN_3_DEMO.md`. Guide design: `BORNPETS_V3_GUIDE_DESIGN.md`.

**Code base:** worktree `BornHack-Cyberegg-v3bg`, branch `ranzbak/bornpets-v3-backgrounds`, from local `main` `5e59574`. Anchors at that commit:
- `src/game/engine/mod.rs`: `GameState` 239 (`pet_kind` 246, `day_min` 301), `Visible` 1157, `sync_clock` 1191, `pet_line` 1201, `visible` 1446, `next_wake_tick` 1476.
- `src/game/engine/solver.rs`: `MAX_EXTRA` 31, `extra_levels` 33 (Dirt row 67), `timer_bound` 191, `seg_len` 289, `sig` 319.
- `src/game/engine/demo.rs`: `MINUTES_PER_DAY` 19, `tick` 831 (moves `day_min` in every mode), `until` 853, `in_night` 859.
- `src/game/engine/status.rs`: `danger_word` 110 (private `poorly` helper), `status_line` 351, `Word` ~150.
- `src/game/engine/needs.rs`: `HEALTH_DANGER_PCT` 24, `build_edges` 109.
- `src/game/engine/thresholds.rs`: field table 68 (`DEMO_*` fields end ~385), `CLASSIC` 394, `CASUAL` 719, `BORNPETS_CFG_KEYS` 1150, `DAY_LAST_MIN` 1909, `validate` 1941.
- `src/game/engine/sim.rs`: `Outcome` 61, `Setup` 107, `run_with` 461, routines `balanced` 693, `absent` 856.
- `src/game/mod.rs`: `status_wants_full_refresh` 477, layout consts 629 to 658 (`SEP_TOP` 34, `SEP_BOT` 111, `HAX_POS` 651, `PET_AREA_TOP` 658), `draw_backing` 670, `BATTERY_Y` 747, `draw_toast` 773, `draw_screen_game` 836 (HAX block ~912, simulator sprite block ~930), `render` 1090 (sprite blit ~1125), `lock_test_state` 1249.
- `src/game/lifecycle.rs`: `REALM` 23, `local_minute` 58, `minute_of_day` 63, `init` 85 (loads kv `realm` 118), `on_wall_clock_set` 310, `start_new_game` 332, `cycle` sync 413, `pet_line` 480, `REALM_DIRTY` 632, `is_hibernating` 980, `new_generation` 1010 (records a living pet through `PetRecord::from_game_state`, `realm.push`, `REALM_DIRTY`), `pet_kind` 1064, `money_enabled` 1128, `realm_count` / `realm_pet` 1190, `save_if_needed` ~1215 (persists `realm` when `REALM_DIRTY`).
- `src/game/engine/mod.rs` Realm: `realm_pending` 406, `PetRecord` 2449 (`pet_kind` 2455), `REALM_MAX_PETS` 2464. `src/game/mod.rs` Gone screen: `GONE_LEFT` 411, `GONE_PROMPT` 412, `screen_words` 420, Gone branch of `draw_screen_game` ~1005. `src/game/realm_view.rs`: the row shows `pet.pet_kind.name()` (= `pet_registry::name_of`).
- `src/platform.rs`: `set_fake_uptime_ms` 13, `unix_secs` 116 (host: system time).
- `src/game/pet_registry.rs`: `install` 83, `name_of` 111, `roster` 124, `is_known` 132. `src/game/pet_select.rs`: titles ~45, `words` ~52, `MODE_NEW_GEN` 67, `open_new_game` 130, `confirm` 179.
- `src/game/sprite_loader.rs`: `init` 93, `count_anim_frames` 166, `blit_file` 305, `blit_pcx_sim` 486.
- `src/bin/embassy.rs`: `pets_cfg::load_and_install` 425, `lifecycle::init` 571, `sprite_loader::init` 572, `render_frame` 769, `game_status` full-refresh choice ~1007. `src/sim_support.rs`: `install_pets_from_repo_cfg` 48, `render` 63.
- `src/guide/shots.rs`: `SHOTS` 346, `run` 532. `src/guide/words.rs`: `build` ~95. `guide/chapters/00-meet-the-pets.typ`, `03-reading-your-pet.typ`.
- `scripts/copy_assets.py`: `update_assets_to` 162, `process_device` 200, `main` 436. `scripts/test_copy_assets.py`.
- `.github/workflows/tests.yml`: matrix line 28, `others` loop (new `tests/*.rs` files run there automatically), `guide` job (last step `make guide`).

## Global Constraints

Every task's requirements include this section.

**Owner rules (verbatim where quoted):**
- "the delta engine as it's written now needs to stay the algorithm to drive progress". No fixed-step loop and no periodic polling. Day and night come from the wall clock through the engine: the sky switch is a timer boundary in `solver::timer_bound`, the floor items are dirt lines in `solver::extra_levels`, and the background's byte is part of `Visible`, so `next_wake_tick` wakes for it. A step loop may exist only as a simulator test oracle (`Driver::Oracle`).
- Spec 14 owner decisions: pet sprites sit on top of combinable backgrounds. Backgrounds are dithered. Day and night follow the wall clock. Ghosting handling stays simple and is added only if it shows up on a badge. Dirt shows "only as discrete objects lying on the floor band, not as specks, sparkles or flies". Hibernating draws no background. There is no sprite redraw: the Cat's sleeping scene (`011002`, `011003`) stays. "The Jellyfish pet is removed as part of the v3 work."
- **TDD, quality first.** Cheaper models implement. The plan carries the design, so every name, position and number below is the decision. Where the plan leaves a choice open, it says so.
- **Test policy:** each task runs **only its own focused tests** locally. No task pushes to CI. The full suite, both firmware builds, the flash gate and the guide build happen once, in Task 9, on the GitHub mirror. Task 10 fixes what Task 9 finds.
- **Local cargo is pinned** (plan 5 runs in parallel on cores 0 and 1): `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo ... -j 2`, one cargo command at a time, also for `cargo sim` and `cargo check`. Never run `cargo fmt` on the whole tree. Never use `git stash`. Firmware is only *checked* locally, and only in the tasks that touch embassy-only code (Tasks 2, 3 and 6): `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo check -j 2 --bin embassy --no-default-features --features embassy-1680-full --target thumbv7em-none-eabihf` (Task 3 also checks `--features embassy-game`).
- **CI and pushing:** CI runs on the GitHub mirror only, and every job keeps `if: github.server_url == 'https://github.com'`. Push `ranzbak/bornpets-v3-backgrounds` to the `github` remote only (public mirror `ranzbak/bornhack-firmware-2026-ce`), never to codeberg, and only in Tasks 9 and 10. Prefix SSH git commands with `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh`.
- **Hand-added PCX files:** 61 files in `assets/to-badge/` are not produced by `asset-gen export`: rows `0x20` to `0x28` for prefixes 00, 01 and 02 (`0020xx` to `0028xx`, `0120xx` to `0128xx`, `0220xx` to `0228xx`), and menu icons `0406xx` and `0407xx`. Never delete them, never call them stale, never regenerate `MANIFEST.TXT` (a regenerated manifest drops them). Edit it by hand. They keep being copied.
- **Per-badge files:** `000000.PCX`, `010000.PCX` and `PETS.CFG` differ per badge, and `BPETS_V3.CFG` may be edited by the player. `--update` never overwrites, edits or deletes them. Neither does the old firmware's `BORNPETS.CFG`, which is not in the assets.
- **FAT12 volume:** 1 MiB, 502 clusters of 2 KiB, 511 root entries. Budget at `5e59574`: `assets/to-badge/` is 422 files and **505 clusters**, 3 more than a fresh volume holds (the pre-existing defect under Goal). The PCX take 490, `MANIFEST.TXT` 6, `BPETS_V3.CFG` 6, `ALARMS.ICS` 2 and `PETS.CFG` 1. The Jellyfish (prefix 08) is 50 PCX in 75 clusters. This plan adds **no** PCX file: every background and floor item is drawn in code.
- **Sprite style:** this plan changes no sprite. A future outline or style change must be uniform all round, in every frame, or not done at all.
- **Battery:** a full refresh flashes the panel and costs battery. Backgrounds add no refresh of their own. They are drawn into frames the pet screen already draws. They add at most 2 partial refreshes a day for the sky switch, plus at most one per floor-item step. They never add a full refresh, and red appears only while the red pet line already asks for one. Task 4 measures the bound. Day/night and backgrounds must not add a refresh per minute.
- **Saves:** the save stays `state_v3`, format 3, 177 bytes. **This plan uses no save byte.** Demo keeps bytes 149 to 159, and bytes 160 to 176 stay reserved for plan 6. Retiring a Jellyfish uses the existing `phase` field of `state_v3` and the existing kv `game`/`realm` record list (`PetRealm`, 10 records). Never write kv `state`, `game/name` or `BORNPETS.CFG`. The retirement does not touch the pet's name key: `new_generation` handles the name as it does for every new egg.
- **Owner answers (2026-11-14):**
  1. "Send it to the Unicorn Realm": a live Jellyfish pet moves to the Realm with its record, then a new egg (Task 6).
  2. The old `PETS.CFG` keeps `8=Jellyfish`: leave the file alone, and have the firmware hide a pet that has no sprites (Task 6).
  3. The sky is plain white while the clock is not set (Task 3).
  4. The poorly mark is the small red cross (Task 3).
  5. The demo 1 % "Only pets hobby" belongs to plan 8 and is out of this plan's scope. It must be a scheduled demo timer, not a per-check dice roll. Task 8 notes this on roadmap row 8.
- **Tunables:** the seven new values are `Thresholds` fields with a Classic and a Casual value and a `BPETS_V3.CFG` key. The template `assets/to-badge/BPETS_V3.CFG` is regenerated (`BPETS_REGEN=1`, plan 3 hook) and the `USER_GAMES.md` key list is updated, in the same commit. Drawing geometry (positions, bitmaps, dither levels) stays const.
- **Flash:** the SSD1680 release image is 866 KiB of the 960 KiB gate. This plan adds an estimated 4 to 6 KiB (see Self-review, Flash). If space runs short, the owner allows removing the Lights Out mini-game (Task 10 only, its own commit).
- **Guide:** this plan updates guide chapter 3 (`guide/chapters/03-reading-your-pet.typ`) and Meet the pets (`00-meet-the-pets.typ`), and the headless shots in `src/guide/shots.rs`. Every new on-screen word is marked with `w()`. Numbers come from `v()`; `lint_digits` rejects bare digits. Read `guide/` and `scripts/build_guide.py` before Task 8. Do not touch `GAME.md` (the final roadmap task rewrites it).
- **Parallel stage (plan 5, Battles):** it touches the battle and friends views, the beacon, `thresholds.rs`, `tests.yml` and guide ch 11. This plan shares these files with it:
  - `thresholds.rs`, `assets/to-badge/BPETS_V3.CFG` and the `USER_GAMES.md` key list. This plan adds its seven fields and keys as one block after the last `DEMO_*` entry in each list. Whichever plan merges second regenerates the template and the key list from the merged table.
  - `tests.yml`: this plan adds one step at the end of the `guide` job and does not touch the matrix.
  - `engine/mod.rs` (`Visible`, `visible`, `pub mod scene;`, `GameState::retire`), `engine/sim.rs` (one `Outcome` field), `lifecycle.rs` (new functions, plus one call in `on_wall_clock_set`), `src/game/mod.rs` (Gone screen lines, HAX) and `BORNPETS_V3_ROADMAP.md` (one note on row 8).
  - `src/guide/shots.rs`: new shots are appended at the end of `SHOTS`.
  - This plan does not touch `battle_view.rs`, `friends_view.rs`, the beacon, guide ch 11 or `guide/lib/social.typ`. The latter's comment "The seven kinds in a row" is left to plan 5.
- Commit footer on every commit: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**Carried from plans 1 to 3:** integer math only in `src/game/engine/` (except `sim.rs`); no `alloc` in engine code; levels are `u16` with `P = 655` per percent; 1 engine minute = 6 ticks; wake cap `MAX_SLEEP_TICKS` 720 ticks (2 h); R15 (code this plan makes dead is removed in the same commit, no commented-out code); R24 (every refusal shows its reason); R26 ("BornPets v3"); R7/R13 (the known failures `name_screen::tests::drops_control_and_beyond_latin1` and the `signed_channel.rs` doctest are not yours).

**Screen geometry** (both panels 152 x 152; the pet area is the sprite's 152 x 75 rows):

| Where | Box |
|---|---|
| Pet area (sprite rows) | x 0 to 151, y 35 to 109 (`PET_AREA_TOP` 35; separators at y 34 and 111) |
| Sky (dithered at night) | y 35 to 99 |
| Floor band (white day and night) | y 100 to 109 |
| HAX balance (top-right, `HAX_POS` (150, 33), text y 33 to 45) | gets a white backing in this plan |
| Toast (top-left, `TOAST_POS` (2, 36), two lines) | has its backing already |
| Status line / note / help (bottom-centre, y 97 to 109, two lines from y 84) | has its backing already, drawn over the floor |
| Battery bar x 148 to 151, y 2 to 16; demo mark `D` x 148 to 151, y 19 to 23 | outside the pet area |
| Sun / moon box | x 129 to 147, y 49 to 67 |
| Poorly cross box | x 140 to 148, y 70 to 78 |
| Gone screen, retired pet: `Moved to the` / `Unicorn Realm` (bold red, centred at y 50 and 66, where `GONE_LEFT` sits) | ≤ 21 columns each |

**Verify (V)**, the last step of Tasks 1 to 8:
1. Run the task's focused tests (the commands are in the task). Nothing else.
2. `git status --short` shows only the files the task names.
3. No push. Report the test names run and their result.

## Review Focus

The failure modes most likely to reach a player unnoticed, most likely first. Each is pinned by a test.

1. **The badge sleeps through a sky or floor change**, or a clock sync leaves the old sky up for hours. A switch or floor line that is not a solver boundary, or a sync that moves the next switch earlier than the scheduled wake. Pinned in Task 2 by `scene_wake_is_exact_on_random_states` (2000 seeded states with `day_min` set or not), `the_sky_switch_is_a_wake`, `a_floor_step_is_a_wake` and `a_clock_jump_redraws`.
2. **The UI becomes unreadable at night**: HAX, toast or status text on the dither, or the sun or cross under the HAX or the battery. Pinned in Task 3 by `hax_reads_at_night`, `ui_boxes_stay_clear` and `background_stays_in_the_pet_area`.
3. **Battery: the background costs flashes.** On the SSD1675 the dither raises changed pixels per idle frame, so the changed-pixel counter promotes to a full (flashing) refresh more often. Red drawn without the red line would also need full refreshes to seat. Pinned in Task 4 by `ssd1675_promotions_stay_bounded`, `the_sky_switch_is_cheap`, `poorly_cross_only_with_the_red_line` and `backgrounds_add_few_wakes`.
4. **Guide pictures and lib tests change with the hour of the build.** The host clock is real time, so a night-time `make guide` would draw night shots. Pinned in Task 2 by `unix_secs_follows_the_fake_clock` and `lock_test_state_pins_noon`, and in Task 8 by `every_shot_is_drawn_at_noon_unless_it_says_night`.
5. **The Jellyfish removal breaks a badge.** Risks:
   - a saved Jellyfish pet shows a blank screen, crashes, is lost without a Realm record, or is recorded twice (once at retirement and again when the player presses Fire);
   - the roster offers a pet without sprites;
   - `--update` deletes a per-badge or hand-added file, or a player's own file at prefix 08;
   - a fresh badge still overflows.

   Pinned in Task 5 by `update_keeps_retired_file_changed_on_badge`, `changes_refuse_protected_names` and `update_removes_a_partial_file_on_no_space`. Pinned in Task 6 by `a_hidden_pet_moves_to_the_realm`, `the_new_egg_records_it_only_once`, `retiring_twice_records_once` and `the_gone_screen_says_where_it_went`. Pinned in Task 7 by `repo_assets_fit_the_volume` and `hand_added_pcx_are_kept`.

## File map

| File | Change |
|---|---|
| `src/game/engine/scene.rs` | create: `Sky`, `BackgroundState`, `pack`, `is_night`, `sky`, `floor_items`, `floor_lines`, `minutes_to_switch`, `background_state`, `wake_view`, `FLOOR_ITEMS` |
| `src/game/engine/mod.rs` | `pub mod scene;`; `Visible` gains `scene::wake_view` (8th element); `GameState::retire` |
| `src/game/engine/solver.rs` | `extra_levels` Dirt gains the five floor lines; `timer_bound` gains `minutes_to_switch` |
| `src/game/engine/status.rs` | `poorly` becomes `pub(crate) fn poorly` (one predicate for the danger word and the cross) |
| `src/game/engine/thresholds.rs` | 7 fields, Classic and Casual values, keys, `validate` rules |
| `assets/to-badge/BPETS_V3.CFG` | regenerated |
| `src/game/engine/sim.rs` | `Outcome::scene_changes_per_day` |
| `src/game/background.rs` | create: `draw_background` and its layer functions, geometry consts, bitmaps, tests (`mod draw`, `mod budget`) |
| `src/game/mod.rs` | `pub mod background;`; `draw_backdrop`; HAX moved after the simulator blit and drawn on a white backing; `render()` draws the backdrop before the sprite; Gone screen `RETIRED_LINES` for a hidden kind, in `screen_words` |
| `src/game/lifecycle.rs` | `background_state`, `clock_sync_needs_redraw`, `on_wall_clock_set` signal, `is_hidden_kind`, `retire_hidden_pet`; `debug_set_dirt_pct` (host only) |
| `src/platform.rs` | host fake wall clock `set_fake_unix` |
| `src/game/pet_registry.rs` | `drop_without_sprites` |
| `src/bin/embassy.rs` | boot order: sprite catalogue, `drop_without_sprites`, then `lifecycle::init`, then `retire_hidden_pet` |
| `BORNPETS_V3_ROADMAP.md` | row 8: the demo hobby moment as a scheduled demo timer (owner answer 5) |
| `src/sim_support.rs` | `install_pets_from_repo_cfg` calls `drop_without_sprites` |
| `scripts/copy_assets.py` | `load_changes`, removals and replacements in `update_assets_to`, `PROTECTED`, `volume_budget` |
| `scripts/test_copy_assets.py` | new tests |
| `assets/asset-changes.txt` | create: `remove` / `replace` lines with sha256 of every shipped version |
| `assets/to-badge/08*.PCX` (50), `assets/bornpets-sponsors-jellyfish.json5`, `assets/bornpets-sprites-jellyfish.png` | delete |
| `assets/to-badge/PETS.CFG`, `assets/to-badge/MANIFEST.TXT` | drop the Jellyfish line and the 50 `08` lines (hand edit) |
| `scripts/bornpets_ai_frames.py` | drop the Jellyfish prompt block |
| `README.md` | export list and prefix table without the Jellyfish; `--update` removals and replacements |
| `tests/engine_v3_scene.rs` | create: engine tests (runs in CI's `others` loop) |
| `src/guide/shots.rs`, `src/guide/words.rs` | fake clock, new shots, new words |
| `guide/chapters/03-reading-your-pet.typ`, `guide/chapters/00-meet-the-pets.typ` | new sections |
| `.github/workflows/tests.yml` | one step in the `guide` job: the `copy_assets` unit tests |
| `USER_GAMES.md` | key list, one "Sky and floor" paragraph, the Jellyfish note |

---

### Task 1: what the background shows (engine), and its tunables

Spec 14.2 (layers), 14.3 (night 21:00 to 07:00, plain while the clock is unknown), 14.5 (floor items 40 / 55 / 70 / 85 / 95).

**Files:**
- Create: `src/game/engine/scene.rs`, `tests/engine_v3_scene.rs`.
- Modify: `src/game/engine/mod.rs` (`pub mod scene;` only), `src/game/engine/status.rs` (`poorly` visibility), `src/game/engine/thresholds.rs`, `assets/to-badge/BPETS_V3.CFG`, `USER_GAMES.md` (key list).

**Interfaces:**
- Consumes: `GameState::{day_min, dirt, health, phase, hibernating}`, `status::status_line`, `GameState::pet_line`, `demo::MINUTES_PER_DAY`.
- Produces:
```rust
// scene.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sky { Plain, Day, Night }       // Plain: wall clock unknown
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackgroundState {
    pub sky: Sky,
    pub floor: u8,      // 0..=FLOOR_ITEMS items on the floor, in slot order
    pub grass: bool,    // healthy and content
    pub poorly: bool,   // the red cross
}
pub const FLOOR_ITEMS: usize = 5;        // crumbs, peel, can, sock, pizza box
impl BackgroundState {
    /// bit 7 set (so Some never packs to 0), bits 0..1 sky (Plain 0, Day 1, Night 2), bits 2..4 floor, bit 5 grass, bit 6 poorly
    pub fn pack(self) -> u8;
}
pub fn is_night(local_min: u16) -> bool;           // window BG_NIGHT_FROM_MIN..BG_NIGHT_TO_MIN, wrapping midnight
pub fn sky(day_min: Option<u16>) -> Sky;           // None -> Plain
pub fn floor_lines() -> [u8; FLOOR_ITEMS];         // the five FLOOR_*_DIRT_PCT, in slot order
pub fn floor_items(dirt: u16) -> u8;               // how many lines satisfy `dirt as i32 > lvl(line) - 1`
/// Minutes until `sky(day_min)` next changes (1..=1440), None while the clock is unknown.
pub fn minutes_to_switch(day_min: Option<u16>) -> Option<u32>;
/// None (draw nothing): not Active or Leaving, or hibernating.
pub fn background_state(s: &GameState) -> Option<BackgroundState>;
/// background_state(s).map_or(0, BackgroundState::pack)
pub fn wake_view(s: &GameState) -> u8;
// status.rs
pub(crate) fn poorly(s: &GameState) -> bool;       // unchanged body, now shared
```
- `grass`: the status line names nothing wrong: `first` is `Word::Content`, `Word::WellRested` or `Word::Asleep`, `second` is `None`, and `danger` is false.
- `poorly`: `status::poorly(s) && s.pet_line().red()`. The cross shows only while the line is already red. In normal play a poorly pet always has the red danger line. In demo the red help line covers it while `DEMO_HELP_HEALTH_PCT` stays above the danger line (default 35 against 30). So the cross never asks for a full refresh of its own (Task 4 pins it).
- `minutes_to_switch`: `until(m, x)` maps 0 to 1440, and the result is the smaller of `until(m, BG_NIGHT_FROM_MIN)` and `until(m, BG_NIGHT_TO_MIN)`. Write a local `until` in `scene.rs`; `demo::until` stays private (planner's call: two lines, no cross-module coupling).

**Tunables** (Classic / Casual; one block after the last `DEMO_*` field, setter and value):

| Key | Classic | Casual | Setter | Meaning |
|---|---|---|---|---|
| `BG_NIGHT_FROM_MIN` | 1260 | 1260 | `clamp_u16(v).min(DAY_LAST_MIN)` | night sky from 21:00 |
| `BG_NIGHT_TO_MIN` | 420 | 420 | same | until 07:00 |
| `FLOOR_CRUMBS_DIRT_PCT` | 40 | 40 | `clamp_pct` | crumbs from this dirt |
| `FLOOR_PEEL_DIRT_PCT` | 55 | 55 | `clamp_pct` | banana peel |
| `FLOOR_CAN_DIRT_PCT` | 70 | 70 | `clamp_pct` | crumpled can |
| `FLOOR_SOCK_DIRT_PCT` | 85 | 85 | `clamp_pct` | sock (the filthy line, `DIRT_DANGER_PCT`) |
| `FLOOR_PIZZA_DIRT_PCT` | 95 | 95 | `clamp_pct` | pizza box |

`validate` adds two rules. The five floor lines must be strictly increasing and at least 1, else all five take the preset values. `BG_NIGHT_FROM_MIN == BG_NIGHT_TO_MIN` sets both to the preset. Casual equals Classic, because these are display lines and not balance (planner's call).

- [ ] **Step 1: write the failing tests** in `tests/engine_v3_scene.rs`, `mod state`:
```rust
#[test] fn night_is_nine_to_seven() {
    // is_night(1259) false, is_night(1260) true, is_night(0) true, is_night(419) true, is_night(420) false
}
#[test] fn an_unknown_clock_is_plain() { /* sky(None) == Sky::Plain; sky(Some(720)) == Day; sky(Some(1380)) == Night */ }
#[test] fn floor_items_build_up_in_order() {
    // floor_items at 39 % == 0, 40 % == 1, 54 % == 1, 55 % == 2, 70 % == 3, 85 % == 4, 95 % == 5, 100 % == 5
    // (levels via pct * P; 40 % exactly counts)
}
#[test] fn switch_minutes_wrap_midnight() {
    // minutes_to_switch(None) == None; Some(1250) -> Some(10); Some(1260) -> Some(600); Some(0) -> Some(420); Some(420) -> Some(840)
}
#[test] fn content_pet_gets_grass_and_a_problem_takes_it() {
    // new_egg(7, PetKind(0)) made Active, Comfort-like levels: background_state().grass;
    // hunger past its band: !grass
}
#[test] fn hibernating_and_eggs_draw_nothing() { /* egg (Hatching) -> None; hibernating -> None; Gone -> None; Leaving -> Some */ }
#[test] fn pack_never_collides() {
    // all 3 * 6 * 2 * 2 states pack to distinct values, none is 0; wake_view of a hibernating pet == 0
}
#[test] fn floor_lines_must_rise() {
    // validate with FLOOR_CAN_DIRT_PCT 50 (below the peel's 55): all five back to 40/55/70/85/95;
    // BG_NIGHT_FROM_MIN == BG_NIGHT_TO_MIN == 600: both back to 1260/420
}
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test engine_v3_scene state::` fails (`scene` missing).
- [ ] **Step 3: implement** `scene.rs`, the visibility change, the seven fields, keys, setters and `validate` rules. Regenerate the template: `BPETS_REGEN=1 CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test engine_v3 template_`. Add the seven keys to the `USER_GAMES.md` key list, one line each with both values.
- [ ] **Step 4: run them and see them pass.** `... --test engine_v3_scene state::` and `... --test engine_v3 template_`: PASS.
- [ ] **Step 5: commit** `feat(bornpets): background state from the clock, dirt and health`.
- [ ] **Step 6: V.**

### Task 2: the background wakes the badge; a fake wall clock for the host

Spec 14.3: "sunrise and sunset are two more kind-B boundaries ... A clock sync that jumps the time re-computes the next boundary." Spec 14.2: "Each step is one dirt threshold ... one wake each at most."

**Files:**
- Modify: `src/game/engine/mod.rs` (`Visible`, `visible`), `src/game/engine/solver.rs` (`extra_levels` Dirt, `timer_bound`), `src/game/lifecycle.rs` (`background_state`, `clock_sync_needs_redraw`, `on_wall_clock_set`), `src/platform.rs` (`set_fake_unix`), `src/game/mod.rs` (`lock_test_state` only).
- Test: `tests/engine_v3_scene.rs` `mod wake`; `src/game/lifecycle.rs` tests; `src/platform.rs` tests.

**Interfaces:**
- Consumes: Task 1 `scene::*`.
- Produces:
```rust
// engine/mod.rs: Visible gains an 8th element, scene::wake_view(self)
// solver.rs: Need::Dirt appends lvl(line) - 1 for each of scene::floor_lines() (Dirt rises to at most 1 + 1 demo + 5 = 7 lines, under MAX_EXTRA 16);
//            timer_bound: if let Some(m) = scene::minutes_to_switch(s.day_min) { plain(m as i64) }  (no regime term reads the sky)
// platform.rs (host builds only)
/// Pin the host wall clock: unix_secs() = base + fake_uptime_ms / 1000 while Some; None restores system time.
pub fn set_fake_unix(base: Option<u32>);
// lifecycle.rs
pub fn background_state() -> Option<engine::scene::BackgroundState>; // cheap read, no update; None without a game
/// A sync from `before` to `after` (local minutes of day) needs an immediate redraw: the sky differs,
/// or the next switch moved earlier by more than a minute.
pub fn clock_sync_needs_redraw(before: Option<u16>, after: Option<u16>) -> bool;
// game/mod.rs, lock_test_state(): also calls platform::set_fake_unix(Some(TEST_NOON_UNIX))
pub(crate) const TEST_NOON_UNIX: u32 = 1_785_578_400; // 2026-08-01 12:00 local at the default TZ_OFFSET_QH 8 (+2 h)
```
- `on_wall_clock_set`: take `s.day_min` before `sync_clock` and after it. If `clock_sync_needs_redraw(before, after)`, signal `crate::TOAST_SIGNAL` (the display loop then runs `cycle`, and the next wake is computed again). The existing catch-up signal stays. Repeater syncs that move the time by a minute or two signal nothing.
- The sky switch only cuts segments. No regime term reads it, so a pet's needs, mood and health do not depend on it (`the_sky_does_not_change_the_pet` pins this).

- [ ] **Step 1: write the failing tests:**
```rust
// tests/engine_v3_scene.rs mod wake
#[test] fn scene_wake_is_exact_on_random_states() {
    // like engine_v3 status_wake::wake_is_exact_on_random_states: 2000 seeds, demo on or off, day_min None or
    // Some(random), dirt random in 30..100 %: next_wake_tick equals the first minute (stepping a clone one minute
    // at a time, settling demo as there) where visible() changes, capped at MAX_SLEEP_TICKS
}
#[test] fn the_sky_switch_is_a_wake() {
    // content pet, sync_clock(t, Some(20:50)), nothing else due: next_wake_tick == last_update_tick + 10 * 6
    // (rem 0); after update to it: background_state().sky == Night
}
#[test] fn a_floor_step_is_a_wake() { /* dirt 39 % rising, no other line near: next_wake_tick lands on the minute dirt reaches 40 % */ }
#[test] fn the_sky_does_not_change_the_pet() {
    // demo off, same pet twice, A with day_min None, B with Some(06:00): update 3 days in one go and in 50 random cuts:
    // needs, mood, health, conditions equal between A and B and between the two cuts
}
#[test] fn washing_clears_the_floor_at_once() { /* dirt 96 %: floor 5; wash() and its action end: floor 0 */ }
// src/game/lifecycle.rs tests
#[test] fn a_clock_jump_redraws() {
    // None -> Some(12:00): true; Some(12:00) -> Some(12:01): false; Some(19:00) -> Some(20:50): true (switch moved earlier);
    // Some(20:50) -> Some(19:00): false (later switch: the planned wake comes first); Some(20:59) -> Some(21:00): true
}
#[test] fn lock_test_state_pins_noon() { /* lock_test_state(); local_minute() == Some(720) */ }
// src/platform.rs tests
#[test] fn unix_secs_follows_the_fake_clock() {
    // set_fake_unix(Some(1000)); set_fake_uptime_ms(5_000): unix_secs() == Some(1005); set_fake_unix(None): system time again
}
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test engine_v3_scene wake::` and `... --lib -- game::lifecycle:: platform::`.
- [ ] **Step 3: implement** as above.
- [ ] **Step 4: run them and see them pass.** The same two commands, plus `... --test engine_v3 status_wake::`, `... --test engine_v3 split_invariance::` and `... --test engine_v3_demo wake::`: PASS. Then the embassy `cargo check` (Global Constraints), because `on_wall_clock_set` is embassy-only.
- [ ] **Step 5: commit** `feat(bornpets): sky switch and floor lines are engine wakes`.
- [ ] **Step 6: V.**

### Task 3: drawing the background

Spec 14.2 layers 1 and 2, drawn before the pet sprite (layer 3) and the UI (layer 4). Spec 14.5: "the same function draws into the simulator display".

**Files:**
- Create: `src/game/background.rs`.
- Modify: `src/game/mod.rs` (`pub mod background;`, `draw_backdrop`, the HAX block, `render()`), `src/game/lifecycle.rs` (`debug_set_dirt_pct`, host only).
- Test: `src/game/background.rs` `mod draw`; `src/game/mod.rs` tests.

**Interfaces:**
- Consumes: Task 1 `BackgroundState`, `Sky`; Task 2 `lifecycle::background_state`, `TEST_NOON_UNIX`.
- Produces:
```rust
// background.rs
pub fn draw_background<D: DrawTarget<Color = TriColor>>(display: &mut D, bg: &BackgroundState) -> Result<(), D::Error>;
pub const SKY_TOP: i32 = 35;        // = game::PET_AREA_TOP
pub const SKY_BOTTOM: i32 = 99;
pub const FLOOR_TOP: i32 = 100;
pub const FLOOR_BOTTOM: i32 = 109;
pub const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
/// Black where BAYER4[y % 4][x % 4] < level; one level per 16-row band from SKY_TOP (the last band runs to SKY_BOTTOM).
pub const NIGHT_LEVELS: [u8; 4] = [10, 9, 8, 7];          // denser towards the top, about 53 % black overall
pub const SUN_MOON_BOX: Rectangle;  // (129, 49) 19 x 19
pub const STARS: [Point; 6] = [Point::new(18, 44), Point::new(50, 40), Point::new(86, 46), Point::new(110, 62), Point::new(28, 74), Point::new(72, 68)];
/// Slot boxes in slot order: crumbs, peel, can, sock, pizza box (bottom row y 109).
pub const FLOOR_SLOTS: [Rectangle; 5] = [
    Rectangle::new(Point::new(4, 107), Size::new(7, 3)),     // crumbs
    Rectangle::new(Point::new(120, 104), Size::new(11, 6)),  // banana peel
    Rectangle::new(Point::new(16, 104), Size::new(10, 6)),   // crumpled can, lying
    Rectangle::new(Point::new(136, 102), Size::new(9, 8)),   // sock
    Rectangle::new(Point::new(29, 105), Size::new(16, 5)),   // pizza box, closed and flat
];
pub const TUFT_XS: [i32; 13];       // 2 + 12 * k, k = 0..13; each tuft 5 x 4 at y 106..109
pub const CROSS_BOX: Rectangle;     // (140, 70) 9 x 9
// game/mod.rs
/// The background behind the pet, when there is one and sprites exist (frame_count() > 0).
pub(crate) fn draw_backdrop<D: DrawTarget<Color = TriColor>>(display: &mut D) -> Result<(), D::Error>;
// lifecycle.rs, #[cfg(any(test, feature = "simulator"))]
pub fn debug_set_dirt_pct(pct: u8);   // sets the live pet's dirt (guide shots and tests only)
```
- **Layers** (each a private function in `background.rs`):
  - `Sky::Plain`: nothing, a white pet area. The badge does not claim a time it does not know (owner answer 3).
  - `Sky::Day`: a white sky with a sun in `SUN_MOON_BOX`: a filled black disc of radius 4 at (138, 58) and eight 2-px rays.
  - `Sky::Night`: the Bayer dither over y `SKY_TOP` to `SKY_BOTTOM`, full width. Then each star is a 5 x 5 black square with a white 3 x 3 plus on it. The moon is a 19 x 19 black square in `SUN_MOON_BOX` with a white crescent: a white disc of radius 7 at (138, 58) minus a black disc of radius 6 at (141, 56).
  - Floor items `0..bg.floor`: each a small const 1-bpp black bitmap on white, drawn inside its `FLOOR_SLOTS` box. The implementer draws the five pictures within the boxes, in black outline with white fill (planner's call on the pixels).
  - Grass, when `bg.grass`: a 5 x 4 black tuft at each `TUFT_XS` whose box meets no box of a shown floor item.
  - Poorly, when `bg.poorly`: `CROSS_BOX` white with a red plus (arms 3 px wide, 7 px long), centred (owner answer 4).
  - The floor band y 100 to 109 stays white at night: items and tufts read on white, and the status backing sits there anyway (planner's call).
- **Hooks:**
  - Firmware `render()`: right after `lifecycle::cycle()` and `prompt_for_name()`, and before the in-game sprite blit, call `draw_backdrop(display)`.
  - Simulator: at the start of the `#[cfg(feature = "simulator")]` sprite block in `draw_screen_game`, call `draw_backdrop(display)`.
  - Move the HAX block from before the simulator block to after it. This also matches the firmware, where the sprite is blitted before `draw_screen_game`. Draw HAX on a white backing: `draw_backing` with the width of `money_buf`, top-left at `HAX_POS.x - width`, y 33.
  - `draw_backdrop` draws nothing while `pet_select`, a mini-game or a takeover alert is active, or while `sprite_loader::frame_count() == 0`. In those cases the screen returns early anyway; the guard keeps the "No sprites on flash" text on white.

- [ ] **Step 1: write the failing tests:**
```rust
// background.rs mod draw (Canvas from game::test_canvas)
#[test] fn background_stays_in_the_pet_area() {
    // every BackgroundState (all 3 * 6 * 2 * 2): no non-white pixel outside x 0..=151, y 35..=109
}
#[test] fn plain_sky_is_white() { /* Plain, floor 0, no grass, not poorly: no non-white pixel at all */ }
#[test] fn night_is_about_half_black_and_denser_on_top() {
    // Night: black share of y 35..=50 > share of y 83..=99; overall share of y 35..=99 outside stars and the moon box in 45..=60 %
}
#[test] fn day_has_a_sun_and_night_a_moon() { /* Day: black pixels inside SUN_MOON_BOX, white elsewhere in the sky; Night: white crescent pixels inside the box */ }
#[test] fn floor_items_are_cumulative_and_never_move() {
    // for n in 0..=5: pixels inside FLOOR_SLOTS[k] are non-white iff k < n; the pixels of slot k are identical for every n > k
}
#[test] fn slots_tufts_and_cross_never_overlap() { /* FLOOR_SLOTS pairwise disjoint; all inside y 100..=109; CROSS_BOX and SUN_MOON_BOX disjoint and inside y 35..=99 */ }
#[test] fn grass_steps_around_the_items() { /* grass with floor 5: no tuft pixel inside any FLOOR_SLOTS box; grass with floor 0: 13 tufts */ }
#[test] fn only_the_cross_is_red() { /* poorly: RED pixels only inside CROSS_BOX; any state without poorly: no RED pixel */ }
// game/mod.rs tests (lock_test_state, simulator)
#[test] fn hax_reads_at_night() {
    // money on, 120 H, sky Night (set_fake_unix to 23:00 local, cycle): every pixel in the HAX backing box is white
    // except the text's own black glyph pixels; the text's pixels equal those drawn on a plain sky
}
#[test] fn ui_boxes_stay_clear() {
    // Night, poorly, floor 5, demo on: battery bar box, demo mark box, toast backing and status backing hold exactly
    // the pixels they hold on a Plain sky
}
#[test] fn backdrop_sits_under_the_sprite() {
    // simulator draw_screen_game at night: a pixel where the idle sprite is opaque white is white, and one where it is
    // transparent shows the dither
}
#[test] fn hibernating_has_no_background() { /* hibernating pet at 23:00: pet area matches the same pet drawn with Sky::Plain */ }
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::background:: game::tests::`.
- [ ] **Step 3: implement** `background.rs`, the hooks, the HAX move and backing, `debug_set_dirt_pct`.
- [ ] **Step 4: run them and see them pass.** The same command (it skips `name_screen::tests::drops_control_and_beyond_latin1`). Run the embassy `cargo check` for `embassy-1680-full` and for `embassy-game`. Run `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo sim -j 2` once to look at day and night (the simulator follows host time): the sprite reads on the dither, the HAX is legible, and the floor items sit clear of a short status line.
- [ ] **Step 5: commit** `feat(game): day and night sky, floor items, grass and the poorly cross`.
- [ ] **Step 6: V.**

### Task 4: the refresh and battery bound

Owner: "keep the refresh count bounded and say how". Spec 14.4: "Backgrounds add no refreshes"; spec 8.3: full refresh "at most once an hour otherwise". The SSD1680 runs every runtime refresh as `RefreshMode::Fast` (no flash). The SSD1675 flashes in two cases. One is the red pet line (`status_wants_full_refresh`, unchanged). The other is the vendor changed-pixel counter, which promotes a partial to a full refresh after `full_after_screens` (default 3) screens of changed pixels: 3 x 152 x 152 = 69 312 pixels. This task measures what the background adds to both.

**Files:**
- Modify: `src/game/engine/sim.rs` (`Outcome::scene_changes_per_day`, counted from the segment snapshots like the other shares), `src/game/background.rs` (`mod budget`).
- Test: `src/game/background.rs` `mod budget`; `tests/engine_v3_scene.rs` `mod budget`.

**Interfaces:**
- Consumes: Tasks 1 to 3; `sprite_loader::blit_pcx_sim`, `anim_files::{anim_filename, frame_count}`, `pacing::IDLE_FRAME_SECS`, `sim_support::install_pets_from_repo_cfg`, `sim::run_with`, `sim::{absent, balanced}`, `Setup`.
- Produces:
```rust
// sim.rs, Outcome
pub scene_changes_per_day: f32,   // times scene::wake_view changed, per day
// background.rs mod budget (test-only helpers)
const SSD1675_FULL_AFTER_PX: u32 = 3 * 152 * 152;   // vendor PartialConfig::default().full_after_screens = 3
const IDLE_FRAMES_PER_DAY: u32 = 24 * 3600 / pacing::IDLE_FRAME_SECS as u32;  // 288
fn changed_px(a: &Canvas, b: &Canvas) -> u32;        // differing pixels in y 35..=109
```
- [ ] **Step 1: write the failing tests:**
```rust
// background.rs mod budget
#[test] fn the_sky_switch_is_cheap() { /* changed_px(Day canvas, Night canvas), no sprite: <= 6_000 */ }
#[test] fn ssd1675_promotions_stay_bounded() {
    // for every pet of the repo roster and every idle-paced anim with >= 2 frames (Idle, Happy, Sleeping, Warning*,
    // Critical*): worst changed_px between consecutive frames, drawn over Day and over Night.
    // added = (IDLE_FRAMES_PER_DAY * (night_worst - day_worst) + 2 * switch_px) / SSD1675_FULL_AFTER_PX
    // total = (IDLE_FRAMES_PER_DAY * night_worst + 2 * switch_px) / SSD1675_FULL_AFTER_PX
    // assert added <= 4 and total <= 24 (the 8.3 hourly cap as a daily bound); eprintln! day_worst, night_worst, added, total
}
#[test] fn poorly_cross_only_with_the_red_line() {
    // 2000 random states, demo on and off: background_state().poorly implies status_wants_full_refresh()'s pet-line
    // term (pet_line().red()) is true; so the cross never needs a full refresh the line does not already take
}
// tests/engine_v3_scene.rs mod budget
#[test] fn backgrounds_add_few_wakes() {
    // run_with(&sim::absent, 10, Delta, 1, Setup { demo: true, clock: true, animated: false }): scene_changes_per_day <= 8,
    // engine_wakes_per_day <= 60 (plan 3's bound still holds)
    // run_with(&sim::balanced, 10, Delta, 1, Setup { clock: true, ..Default::default() }): scene_changes_per_day <= 6
}
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::background::budget` and `... --test engine_v3_scene budget::`.
- [ ] **Step 3: implement** the `Outcome` field and the helpers. Do not tune the drawing to pass. If a bound fails, report the numbers: Task 10 owns the fix.
- [ ] **Step 4: run them and see them pass.** The same commands, plus `... --test engine_v3_demo demo_smoke`: PASS. Copy the `eprintln!` numbers into the task report.
- [ ] **Step 5: commit** `test(game): background refresh and wake budget`.
- [ ] **Step 6: V.**

### Task 5: `copy_assets.py --update` removes and replaces shipped files

Roadmap row 7: assets reach existing badges through `copy_assets.py --update`, "extend it for replacements". Today `--update` only adds missing files. On a full volume it stops on the first "no space" error and leaves a partial file (the pre-existing defect under Goal).

**Files:**
- Modify: `scripts/copy_assets.py`, `scripts/test_copy_assets.py`, `.github/workflows/tests.yml` (one step), `README.md` (the `--update` paragraph).
- Create: `assets/asset-changes.txt` (header comment only; Task 7 fills it).

**Interfaces:**
- Consumes: nothing new.
- Produces:
```python
CHANGES_FILE = REPO_ROOT / "assets" / "asset-changes.txt"   # outside to-badge: never copied to a badge
PROTECTED = frozenset({"000000.pcx", "010000.pcx", "pets.cfg", "bpets_v3.cfg", "bornpets.cfg"})  # folded names
VOLUME_CLUSTERS, CLUSTER_BYTES, ROOT_ENTRIES = 502, 2048, 511
def load_changes(path) -> list[tuple[str, str, frozenset[str]]]:
    """Lines `remove NAME SHA256 [SHA256 ...]` and `replace NAME SHA256 [SHA256 ...]`; '#' comments.
    The hashes are the shipped versions a badge may hold. ValueError on an unknown op, a missing hash,
    or a PROTECTED name."""
def update_assets_to(mount_point, assets_dir, changes=()) -> dict:
    """Removals, then replacements, then the missing files (so space is freed first).
    Returns {"removed": [...], "replaced": [...], "added": [...], "kept": [...], "no_space": [...]}."""
def volume_budget(assets_dir) -> tuple[int, int]:  # (clusters, root entries) a fresh copy of assets_dir needs
```
- A removal deletes the volume file only when its sha256 is one of the listed hashes. Otherwise the file goes to `kept`: a player may have put their own pet at that prefix. A replacement overwrites only a listed old version, written to `NAME.TMP` and then `os.replace`. A replacement whose asset file is missing in `assets_dir` raises `ValueError`. Name matching folds case, as today. If an add or a replacement hits `OSError` with `errno.ENOSPC`, the partial file is deleted (for a replacement, the `NAME.TMP` file, so the old file stays), and the name goes to a fifth list, `"no_space"`. The run then goes on with the remaining files. `process_device` passes `load_changes(CHANGES_FILE)` and prints the counts, with a loud `WARN` line naming each `no_space` file.
- CI: append `- run: python3 -m unittest discover -s scripts -p 'test_copy_assets.py'` to the `guide` job, after `test_build_guide.py`. It is a host-only Python step and keeps the job's `if:` guard.

- [ ] **Step 1: write the failing tests** in `scripts/test_copy_assets.py`:
```python
def test_update_removes_retired_file_with_known_hash(self): ...   # vol 080100.PCX == shipped bytes -> deleted, in "removed"
def test_update_keeps_retired_file_changed_on_badge(self): ...    # vol 080100.PCX other bytes -> kept, in "kept"
def test_update_replaces_only_a_known_old_version(self): ...      # MANIFEST.TXT old shipped -> new content; edited -> kept
def test_changes_refuse_protected_names(self): ...                # remove/replace of PETS.CFG, 000000.PCX, 010000.PCX, BPETS_V3.CFG, bornpets.cfg -> ValueError
def test_update_removes_before_it_adds(self): ...                 # record os.remove and the add copy: every remove precedes every add
def test_update_without_changes_behaves_as_before(self): ...      # changes=() -> identical to today's result plus empty lists
def test_update_removes_a_partial_file_on_no_space(self): ...     # patch copyfileobj to write 10 bytes then raise OSError(ENOSPC): no file left at dst, name in "no_space", later files still added
def test_volume_budget_counts_clusters_and_entries(self): ...     # temp dir: 1 B, 2048 B, 2049 B files -> (1 + 1 + 2, 3)
```
- [ ] **Step 2: run and see them fail.** `python3 -m unittest discover -s scripts -p 'test_copy_assets.py'`.
- [ ] **Step 3: implement**, then the README paragraph: what `--update` removes and replaces, that it keeps changed files, and the protected names.
- [ ] **Step 4: run them and see them pass.** The same command: PASS (the existing tests too).
- [ ] **Step 5: commit** `feat(assets): copy_assets --update removes and replaces shipped files`.
- [ ] **Step 6: V.**

### Task 6: pets without sprites leave the roster; a live one moves to the Unicorn Realm

Spec 14.5: "check what a badge with a saved Jellyfish pet does on boot (it must fall back to pet selection, not crash)". Owner answer 1 (2026-11-14): "send it to the Unicorn Realm". The pet moves to the Realm with its record, then the player starts a new egg. This goes through the existing Gone/Realm path: the record is pushed as `new_generation` pushes a "Reset Pet" (`PetRecord::from_game_state`, `realm.push`, `REALM_DIRTY`), the pet becomes Gone, and the existing Gone screen leads through pet selection to a new egg. No save byte is used. The only kv keys written are the existing `state_v3` and `realm`.

**When it happens:** at every boot, after the sprite catalogue, a live pet whose kind has no idle sprite on the badge is retired. A pet retires once: after that it is Gone. In practice this is the first boot of the new firmware on a badge without Jellyfish sprites. That covers a fresh asset copy, and a badge after `copy_assets.py --update` removed them (the catalogue is read at boot, so the next boot after the update). A badge with the new firmware whose Jellyfish files are still on flash keeps its Jellyfish: the files and the `PETS.CFG` line still make it a valid pet.

**What the player sees:** the Gone screen, in bold red as today. In place of `GONE_LEFT` it shows `Moved to the` / `Unicorn Realm`, which is true: the record has been pushed. The existing `Press Fire` / `for a new egg` follows. Fire opens pet selection (`Choose your Pet`, then the money choice) as after any pet that left. The Gone screen already gets the full tri-colour refresh (`status_wants_full_refresh`, Gone), so this adds nothing new.

**The Realm record keeps its kind name:** yes. `drop_without_sprites` removes the pet from the selectable roster but keeps its `PETS.CFG` name. An old badge keeps `8=Jellyfish` (owner answer 2: `--update` never edits `PETS.CFG`), so `realm_view` shows "<name> (Jellyfish) - <age>". Only if the player deletes that line does the kind show as "Pet" (`UNKNOWN_NAME`). A fresh badge has no Jellyfish records.

**Files:**
- Modify: `src/game/pet_registry.rs`, `src/game/engine/mod.rs` (`GameState::retire`; `#[derive(PartialEq, Eq, Debug)]` on `PetRecord` if missing), `src/game/lifecycle.rs`, `src/game/mod.rs` (Gone screen lines, `screen_words`), `src/bin/embassy.rs` (boot order), `src/sim_support.rs`.
- Test: `src/game/pet_registry.rs`, `src/game/lifecycle.rs` and `src/game/mod.rs` tests.

**Interfaces:**
- Consumes: `sprite_loader::count_anim_frames`, `anim_files::anim_id_for(DisplayAnim::Idle)`, `PetRecord::from_game_state`, `REALM`, `REALM_DIRTY`, `pet_name_bytes_sync`, `pet_select::open_new_generation`.
- Produces:
```rust
// pet_registry.rs
/// Once, after the sprite catalogue: drop from the selectable roster every pet with no idle frame.
/// Its PETS.CFG name stays known to `name_of`, so Unicorn Realm records keep it.
pub fn drop_without_sprites(has_sprites: impl Fn(u8) -> bool);
fn drop_from(reg: &mut Registry, has_sprites: impl Fn(u8) -> bool); // the pure part, unit-tested on a local Registry
// engine/mod.rs
/// The pet leaves for good now: phase Gone, no running action, not hibernating, realm_pending false
/// (the caller has pushed the record); request_save().
pub fn retire(&mut self);
// lifecycle.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Retired { Nothing, ToRealm, Egg }
/// A kind the badge cannot show: not in the selectable roster, while the roster has other pets.
pub fn is_hidden_kind(kind: engine::PetKind) -> bool;
/// At boot, after drop_without_sprites and lifecycle::init:
/// - no game, an empty roster, a known kind, or already Gone: Nothing;
/// - Hatching (an egg never lived, as new_generation's rule): pet_select::open_new_generation(), no record: Egg;
/// - Active or Leaving (hibernating and demo pets included): push PetRecord::from_game_state unless the realm's
///   last record already equals it (a power cut between the realm and state writes), set REALM_DIRTY, retire(): ToRealm.
pub fn retire_hidden_pet() -> Retired;
// game/mod.rs
const RETIRED_LINES: [&str; 2] = ["Moved to the", "Unicorn Realm"]; // drawn at y 50 and 66 when is_hidden_kind(pet_kind())
```
- Boot order in `src/bin/embassy.rs`:
  1. `pets_cfg::load_and_install` (as today).
  2. `sprite_loader::init().await`, moved before `lifecycle::init`.
  3. `pet_registry::drop_without_sprites(|id| sprite_loader::count_anim_frames(id, idle) > 0)`.
  4. `lifecycle::init().await`. It loads the save and the kv `realm`, so the push lands on the loaded Realm.
  5. `lifecycle::retire_hidden_pet()`, with a `defmt::info!` of the result.

  Update the `REGISTRY` safety comment: it gets two boot-time writes, both before any reader. `sim_support::install_pets_from_repo_cfg` calls `drop_without_sprites` with the simulator's `count_anim_frames`.
- `save_if_needed` is unchanged. It already writes `realm` when `REALM_DIRTY` is set, and then `state_v3`, so the retirement persists within one save cycle. If the power is cut between the two writes, the next boot retires the pet again, and the last-record check stops a second record.
- Gone-screen text: when `is_hidden_kind(pet_kind())`, draw `RETIRED_LINES` where `GONE_LEFT` goes, whatever `frame_count()` says. A hidden kind has no farewell sprite to show. Otherwise the screen is as today. A pet that left by itself before the update also shows the lines, which is true: every pet that leaves is recorded in the Realm.

- [ ] **Step 1: write the failing tests:**
```rust
// pet_registry.rs
#[test] fn pets_without_sprites_leave_the_roster_but_keep_their_name() {
    // local Registry filled with 0=A, 7=B, 8=Jellyfish; drop_from(&mut reg, |id| id != 8):
    // ids == [0, 7]; the name lookup for 8 still gives "Jellyfish"; 8 is not selectable
}
// lifecycle.rs (lock_test_state; a roster without 8 installed, e.g. install_pets_from_repo_cfg after Task 7 or the test roster)
#[test] fn a_hidden_pet_moves_to_the_realm() {
    // start_new_game(PetKind(8), true, false), hatch, set_pet_name(b"Jelly"), skip 600 min; n = realm_count();
    // retire_hidden_pet() == Retired::ToRealm; realm_count() == n + 1 (or 10 when full: oldest dropped, as push does);
    // the newest realm_pet: pet_kind == PetKind(8), generation and age_ticks equal the pet's, name "Jelly";
    // display_anim() == Gone; is_started() false (the Gone rule at lifecycle 209)
}
#[test] fn the_new_egg_records_it_only_once() {
    // after a_hidden_pet_moves_to_the_realm's steps: new_generation(PetKind(0), true, false) -> realm_count() unchanged,
    // the live pet is an egg of kind 0
}
#[test] fn retiring_twice_records_once() {
    // retire_hidden_pet() twice -> one record (the second call sees Gone: Nothing);
    // power-cut case: push the pet's own record by hand, keep it Active, retire_hidden_pet() -> ToRealm, still one record
}
#[test] fn a_hidden_egg_goes_back_to_pet_selection() {
    // start_new_game(PetKind(8), ..) still Hatching: Retired::Egg, realm_count() unchanged, pet_select::is_active()
}
#[test] fn hibernating_and_demo_pets_move_too() { /* kind 8, hibernating: ToRealm, !is_hibernating(); kind 8 in demo: ToRealm */ }
#[test] fn a_known_pet_is_left_alone() { /* PetKind(0) Active: Nothing, phase and realm unchanged */ }
#[test] fn nothing_happens_without_a_roster() { /* is_hidden_kind is false while the roster is empty (pure check on an empty slice) */ }
#[test] fn retiring_uses_only_state_v3_and_realm() {
    // engine::SAVE_KEY == "state_v3"; the retire path's source (include_str of lifecycle.rs between the
    // retire_hidden_pet markers) names no "\"state\"", "game/name" or "BORNPETS.CFG"
}
// game/mod.rs (simulator, lock_test_state)
#[test] fn the_gone_screen_says_where_it_went() {
    // retired kind-8 pet: draw_screen_game -> RED pixels in the text rows of y 50 and 66 and of GONE_PROMPT;
    // a kind-0 pet that went Gone: no pixels in the RETIRED_LINES rows (frame_count() is u8::MAX on the simulator);
    // screen_words() contains both RETIRED_LINES; each line <= 21 columns
}
```
- [ ] **Step 2: run and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --lib -- game::pet_registry:: game::lifecycle:: game::tests::`.
- [ ] **Step 3: implement** as above.
- [ ] **Step 4: run them and see them pass.** The same command, then the embassy `cargo check`.
- [ ] **Step 5: commit** `feat(game): a pet without sprites moves to the Unicorn Realm`.
- [ ] **Step 6: V.**

### Task 7: remove the Jellyfish

Spec 14.5: "delete `08xxxx.PCX`, its `PETS.CFG` line and `MANIFEST.TXT` entries and its sprite sheet sources".

**Files:**
- Delete: `assets/to-badge/08*.PCX` (50 files), `assets/bornpets-sponsors-jellyfish.json5`, `assets/bornpets-sprites-jellyfish.png`.
- Modify: `assets/asset-changes.txt`, `assets/to-badge/PETS.CFG` (drop `8=Jellyfish`), `assets/to-badge/MANIFEST.TXT` (drop the 50 lines that start with `08`), `scripts/bornpets_ai_frames.py` (drop `JELLYFISH_IDENTITY`, `JELLYFISH_FREEDOM`, `jellyfish_anchor_prompt` and their uses), `README.md` (export list and prefix table), `src/game/pet_registry.rs` (the doc comment "uses `0..=2` and `5..=8`" becomes `5..=7`), `scripts/test_copy_assets.py`, `USER_GAMES.md` (one sentence: the Jellyfish has left).

**Interfaces:**
- Consumes: Task 5 `load_changes`, `volume_budget`, `PROTECTED`; Task 6 (a badge that still has the files keeps working).
- Produces: a 372-file, 429-cluster `assets/to-badge/`; `asset-changes.txt` with 50 `remove` lines and one `replace MANIFEST.TXT` line.

- [ ] **Step 1: write the failing tests** in `scripts/test_copy_assets.py`:
```python
def test_repo_assets_fit_the_volume(self): ...       # volume_budget(ASSETS_DIR): clusters <= 502 - 16 (keep 16 clusters for later plans), entries <= 511
def test_hand_added_pcx_are_kept(self): ...          # 61 files match ^0[012]2[0-8][0-9A-F]{2}\.PCX$ or ^040[67][0-9A-F]{2}\.PCX$; none is in a remove line
def test_repo_changes_load_and_touch_no_protected_file(self): ...  # load_changes(CHANGES_FILE) parses; no name in PROTECTED
def test_no_jellyfish_left(self): ...                # no 08*.PCX in to-badge; PETS.CFG has no "8="; MANIFEST.TXT has no line starting "08"
```
- [ ] **Step 2: run and see them fail.** `python3 -m unittest discover -s scripts -p 'test_copy_assets.py'` (budget: 505 clusters; the Jellyfish is still there).
- [ ] **Step 3: record the hashes before deleting.** For every `assets/to-badge/08*.PCX`, collect the sha256 of every version git has ever held. List the blobs with `git log --all --format= --raw --no-abbrev -- <path>` (the new-blob column) and hash each with `git cat-file blob <id> | sha256sum`. Write one line `remove <NAME> <sha256> ...` per file. Do the same for `MANIFEST.TXT`: one `replace MANIFEST.TXT <sha256> ...` line listing every historic version (never the new one).
- [ ] **Step 4: remove.** `git rm` the 50 PCX and the two sources. Edit `PETS.CFG` and `MANIFEST.TXT` by hand (`grep -v '^08'` for the manifest; never regenerate it). Then the script, the README export list (drop the last `jellyfish.json5` line: it is last, so no prefix renumbers) and the prefix table, the doc comment and the `USER_GAMES.md` sentence.
- [ ] **Step 5: run them and see them pass.** The same command: PASS. `python3 -I scripts/bornpets_ai_frames.py --help` still runs. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test guide_shots sprites_cover_the_roster_without_sponsors`: PASS (roster without 08).
- [ ] **Step 6: commit** `chore(assets): remove the Jellyfish pet` (sources, files, roster line, manifest lines, asset-changes).
- [ ] **Step 7: V.**

### Task 8: the player's guide

Guide rule: each plan updates its chapter; every new on-screen word is marked; `make guide` is green.

**Files:**
- Modify: `src/guide/shots.rs`, `guide/chapters/03-reading-your-pet.typ`, `guide/chapters/00-meet-the-pets.typ`, `tests/guide_shots.rs`, `BORNPETS_V3_ROADMAP.md` (row 8 note). `src/guide/words.rs` needs no change: `RETIRED_LINES` reach `words.json` through `screen_words()` (Task 6).

**Interfaces:**
- Consumes: Task 2 `platform::set_fake_unix`; Task 3 `lifecycle::debug_set_dirt_pct`; Task 6 `lifecycle::retire_hidden_pet`, `RETIRED_LINES`; Task 1 keys through `v()`.
- Produces:
```rust
// shots.rs
pub const SHOT_NOON_UNIX: u32 = 1_785_578_400;   // 12:00 local at TZ_OFFSET_QH 8 (same instant as TEST_NOON_UNIX)
pub const SHOT_NIGHT_UNIX: u32 = 1_785_618_000;  // 23:00 local, the same day
// run(): set_fake_unix(Some(SHOT_NOON_UNIX)) next to set_fake_uptime_ms(0)
// new shots, appended at the end of SHOTS:
//   "day"        content pet at noon                  expect: background_state() sky Day and grass
//   "night"      content pet, Do(set_night) (set_fake_unix(Some(SHOT_NIGHT_UNIX)) then a cycle)  expect: sky Night
//   "floor"      content pet, Do(|| debug_set_dirt_pct(96))   expect: floor == 5
//   "to-the-realm" Start with PetKind(8) (no sprites since Task 7), hatch, Do(retire) (retire_hidden_pet() == ToRealm)
//                  expect: display_anim() == Gone and lifecycle::is_hidden_kind(lifecycle::pet_kind())
```
- **Every shot changes**, because the game screen now has a background. The noon default keeps them stable: a day sky with a sun, plus grass and floor items wherever the shot's state has them. Check the "content" shot that `content-marks` annotates (marks A to F). If the sun lands on a mark label, move the label offsets in `guide/lib/booklet.typ`, not the sun.
- **Chapter 3**, a new section `== Sky and floor` after "The screen". It covers:
  - Day (white, a sun) and night (a dark dithered sky, a moon and stars), night from `#v("BG_NIGHT_FROM_MIN")` to `#v("BG_NIGHT_TO_MIN")` by the badge's clock.
  - Without a set clock the sky stays plain white.
  - The night sky is not the pet's bedtime: a pet can be up at night or nap by day.
  - The floor: a table of the five items with `#v("FLOOR_*_DIRT_PCT")`. Each appears in its own spot and never moves, and a wash clears them all.
  - Grass when nothing is wrong; a small red cross when the pet is poorly (it comes with the red line).
  - A hibernating pet has no sky.
  - The sky costs no extra screen flashes (one sentence; no numbers that are not `v()`).
  - Pictures: `screen("day")`, `screen("night")`, `screen("floor")`.
- **Meet the pets**: one short paragraph. The jellyfish has swum off to the Unicorn Realm. On a badge whose pet was a jellyfish, that pet moves to the Realm with its record the first time the badge starts without jellyfish pictures. The screen says #w("Moved to the") #w("Unicorn Realm"), and #w("Press Fire") starts a new egg. The jellyfish stays in the Realm list under its name. Add `screen("to-the-realm")`. The roster grid drops the jellyfish by itself (`roster.json` from `PETS.CFG`).
- **Roadmap note** (owner answer 5): in `BORNPETS_V3_ROADMAP.md`, add to row 8's "Ships" cell: "demo pet's occasional Only pets hobby moment (spec 8.3), as a scheduled demo timer (`demo::timers`), never a per-check dice roll". Commit it with this task.

- [ ] **Step 1: write the failing test** in `tests/guide_shots.rs`:
```rust
#[test] fn every_shot_is_drawn_at_noon_unless_it_says_night() {
    // two separate runs of `guide shot content`: the same non_white count and byte-identical PNGs (the fake clock,
    // not the host's, picks the sky); `guide shot night` has non_white >= content's + 3_000 (the dither);
    // `guide shot day` (a named copy of a content pet at noon, for ch 3) has the same non_white as `content`
}
```
`every_shot_reaches_its_state` needs no change: it finds the four new shots (`day`, `night`, `floor`, `to-the-realm`) through `guide shots`.
- [ ] **Step 2: run and see it fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test guide_shots`.
- [ ] **Step 3: implement** the shots and the fake clock in `run`, then the two chapters.
- [ ] **Step 4: run and see it pass.** The same command: PASS. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 make guide` (it builds the guide bin and checks marks, digits and pictures): green. Look at the PDF pages for chapters 0 and 3.
- [ ] **Step 5: commit** `docs(guide): sky and floor in ch3, the jellyfish moves to the Realm`.
- [ ] **Step 6: V.**

### Task 9: regression and integration on CI

One push, the whole suite. Nothing changes in this task.

**Files:** none (report only).

**Interfaces:**
- Consumes: Tasks 1 to 8.
- Produces: the regression report (in the task report, not a file):
  - every CI job's result, with the failing test names and their assert messages;
  - the Task 4 numbers from CI's output (`day_worst`, `night_worst`, added and total SSD1675 promotions a day, scene changes a day);
  - the SSD1680 release image size;
  - the `volume_budget` of `assets/to-badge/`.

- [ ] **Step 1:** run locally, one command at a time: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test -j 2 --features simulator --test engine_v3_scene`, then `... --test engine_v3 status_wake::`, `... --test engine_v3 split_invariance::`, `... --lib -- game::background::`, and `python3 -m unittest discover -s scripts -p 'test_copy_assets.py'`. Note failures; fix nothing yet.
- [ ] **Step 2:** `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh git push github ranzbak/bornpets-v3-backgrounds`.
- [ ] **Step 3:** `gh run list -R ranzbak/bornhack-firmware-2026-ce -b ranzbak/bornpets-v3-backgrounds -L 1 --json databaseId,headSha`. The `headSha` must equal `git rev-parse HEAD`.
- [ ] **Step 4:** `gh run watch <id> -R ranzbak/bornhack-firmware-2026-ce --exit-status`, then `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --json conclusion,jobs`. For each failed job: `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --log-failed`.
- [ ] **Step 5:** check the following against the plan's bounds.
  - Both firmware builds pass, and the flash gate holds: the SSD1680 release image is ≤ 960 KiB. If it is over, removing Lights Out goes to Task 10 as its own item.
  - The guide job is green, including the new `copy_assets` step.
  - The Task 4 numbers are within their bounds.
- [ ] **Step 6:** report: green or the list of failures, the numbers, the image size. If everything is green, the coordinator may DFU-flash `fw-1680-release` to the owner's SSD1680 badge (owner allowance after review and green CI) so the owner can look for ghosting at the day/night switch (spec 14.4) and, on a badge that still has a Jellyfish save, check that it moves to the Unicorn Realm after `--update` and a reboot. Task 10 then records "nothing to fix" and the final numbers.

### Task 10: bug-fix round

Fix what Task 9 found, one item at a time.

**Files:** as each fix needs.

**Interfaces:**
- Consumes: the Task 9 report (and the owner's badge check, if any).
- Produces: fixes and a final green CI run.

For each failure:
- [ ] **Step 1:** reproduce it with a focused test (an existing one, or a new short one in the matching module). See it fail.
- [ ] **Step 2:** fix the cause (superpowers:systematic-debugging: root cause first).
- [ ] **Step 3:** run that focused test and the module it sits in. See them pass.
- [ ] **Step 4:** commit `fix(game): <what>` (one commit per item).

Fix rules:
- `ssd1675_promotions_stay_bounded` over its bound: first lower `NIGHT_LEVELS` towards `[8, 7, 6, 5]` (sparser), then end the dither at y 83 (an open band above the floor). Never add a full refresh. Report the before and after numbers.
- Ghosting seen on the badge at the switch (spec 14.4): only then add one full refresh at the sky switch on the SSD1675 (2 a day), in its own commit. The SSD1680 needs none (its delta de-ghosts).
- Flash over 960 KiB: remove the Lights Out mini-game in its own commit (owner's allowance), with its menu row, cooldown, docs, guide text and tests.
- A wake miss: fix the line or timer in `solver`, never add a periodic wake.

After the last fix:
- [ ] **Step 5:** push and watch CI as in Task 9, Steps 2 to 4. Repeat this task until CI is green. Report the final numbers and the image size.

## On-screen words

| Where | Text |
|---|---|
| Gone screen, a pet whose kind has no sprites (bold red, in place of `Your pet has left`) | `Moved to the` / `Unicorn Realm` |

The background has no text. The HAX balance, the toasts and the status line keep their words; HAX gains only a white backing. The retired pet's screen keeps the existing `Press Fire` / `for a new egg`, and pet selection after it is unchanged.

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| 14.1 transparency audit; Cat sleeping scene kept; Slug hibernation has no background | T3 (`hibernating_has_no_background`); no sprite change |
| 14.2 layer 1: day white with a sun, night dither denser on top, white stars, moon, not solid black | T1 (`sky`), T3 (`night_is_about_half_black_and_denser_on_top`, `day_has_a_sun_and_night_a_moon`) |
| 14.2 floor items 40 / 55 / 70 / 85 / 95, fixed slots, cumulative, nothing moves, wash clears in one refresh | T1 (`floor_items`), T2 (`a_floor_step_is_a_wake`, `washing_clears_the_floor_at_once`), T3 (`floor_items_are_cumulative_and_never_move`) |
| 14.2 healthy and content: grass | T1 (`grass`), T3 |
| 14.2 poorly: red cross (the "or" of the spec; owner answer 4) | T1 (`poorly`), T3 (`only_the_cross_is_red`), T4 |
| 14.2 order: background, sprite, status and UI | T3 (`backdrop_sits_under_the_sprite`, `ui_boxes_stay_clear`) |
| 14.2 hibernating draws no background | T1 (`hibernating_and_eggs_draw_nothing`), T3 |
| 14.2 drawn in code, no images | T3; no PCX added (FAT12 budget) |
| 14.2 "last mess" field (optional later) | not done: it needs save bytes, and the owner asked for none unless needed |
| 14.3 local time from `unix_now` + TZ; night 21:00 to 07:00; plain white while unknown (owner answer 3) | T1 (keys, `is_night`, `sky`), T2 (engine `day_min` via `sync_clock`) |
| 14.3 sunrise and sunset as kind-B boundaries; a sync recomputes the next one | T2 (`timer_bound`, `the_sky_switch_is_a_wake`, `a_clock_jump_redraws`) |
| 14.3 night independent of the pet's sleep | T1 (`sky` reads only the clock), T8 (guide sentence) |
| 14.4 no added refreshes; switch and overlays are partial; full at the switch only if ghosting is seen | T4 (bounds), T10 (ghosting rule) |
| 14.5 `background.rs`, `draw_background`, dither helper, called before the sprite blit | T3 |
| 14.5 "seeded speck positions" | dropped: the owner's floor-item decision replaced specks; stars use fixed positions |
| 14.5 engine sunrise/sunset in `next_wake_tick`; dirt thresholds in the kind-A table | T2 (as tunable lines in `extra_levels`, where the solver keeps tunable lines; 85 is the `DIRT_DANGER_PCT` edge already) |
| 14.5 Jellyfish: files, manifest lines, sources removed; old `PETS.CFG` line left alone (owner answer 2); a saved Jellyfish moves to the Unicorn Realm with its record, then a new egg (owner answer 1, replaces the spec's "fall back to pet selection") | T5, T6 (`a_hidden_pet_moves_to_the_realm` and the rest), T7, T8 (`to-the-realm` shot) |
| 14.5 simulator draws the same function | T3 (`draw_backdrop` in both paths) |
| Roadmap 7: assets reach badges through `copy_assets.py --update`, extended for replacements | T5 (removals and hash-guarded replacements), T7 (the change list) |
| Roadmap 7: updates guide ch 3 | T8 (plus Meet the pets, as briefed) |
| 8.3 calm idle (5-minute idle frame) | ruled out: already shipped by plan 2 Task 3 (`pacing::IDLE_FRAME_SECS` 300, for every pet) |
| 8.3 full refresh at most once an hour | included as a measured bound: T4 `ssd1675_promotions_stay_bounded` (≤ 24 a day from idle frames); the SSD1680 has no runtime full refresh. The 8.3 "full refresh at sleep and wake" is not added (spec 14.4: only if ghosting shows) |
| 8.3 1 % "Only pets" hobby moment | moved to plan 8 as a scheduled demo timer, never a per-check dice roll (owner answer 5); T8 adds the note to roadmap row 8 |

**Resolved conflicts and gaps (planner's calls):**
1. **The background is an engine function.** It must be in `Visible` for exact wakes, so `scene.rs` lives in the engine (integer math, no drawing) and `background.rs` in the UI. The spec's `BackgroundState` name is kept.
2. **Plain sky while the clock is unknown.** Spec: "the base layer is plain day". Plain means white with no sun, so the badge never claims a time it does not know. Owner answer 3.
3. **The floor band stays white at night.** The status backing covers its centre anyway, and items and tufts read on white. The slots sit at the sides (x 4 to 44 and 120 to 144), clear of a status line of up to 13 columns. A longer line covers some items: that is the spec's layer order (UI on top).
4. **The cross shows only with the red line** (`poorly && pet_line().red()`). Red on the SSD1675 seats only through the full tri-colour refresh, which the red line already triggers. So the cross never causes a flash. A demo config with `DEMO_HELP_HEALTH_PCT` under the danger line simply shows no cross.
5. **HAX gets a white backing and is drawn after the sprite in the simulator too.** Without the backing it is unreadable on the night sky. The move also makes the simulator match the firmware draw order.
6. **Seven tunables.** The night window and the five floor lines are numbers a player may want to change, for example for festival hours. Casual equals Classic. The geometry stays const.
7. **The Jellyfish stays in old badges' `PETS.CFG`.** The file is per badge and `--update` never edits it. The firmware hides any pet without sprites, so the line does no harm and keeps the Realm's names (owner answer 2).
8. **Hash-guarded asset changes.** A removal or replacement touches only a byte-identical shipped version, so a player's own prefix-08 pet survives. The 61 hand-added PCX are never listed (T7 test).
9. **A saved Jellyfish moves to the Unicorn Realm** (owner answer 1). At the first boot without its sprites, an Active or Leaving pet (hibernating and demo included) is pushed to the Realm with its record and set Gone. The screen says "Moved to the / Unicorn Realm", and Fire starts a new egg. An unhatched egg goes straight back to pet selection with no record. The push is skipped when the Realm's last record already equals the pet, so a power cut between the `realm` and `state_v3` writes never records it twice. No save byte is used.
10. **The FAT12 overflow today.** `assets/to-badge/` at `5e59574` needs 505 of 502 clusters, so a fresh `copy_assets.py` copy runs out of space on its last file (`PETS.CFG`, last in sort order). Existing badges are hit too: one holding every current file is at 505, so `--update` fails "no space" on its last missing file and leaves a partial file. Removing the Jellyfish fixes the count (429). T5 runs removals before additions and deletes a partial file on ENOSPC. T7 keeps 16 clusters spare as a guard for later plans.
11. **The SSD1675's red-line flashes are not changed.** While the red line is up, every changed frame (an idle frame every 5 minutes) runs a full refresh. That predates this plan and is unchanged; it is noted for the owner, not fixed here.
12. **A remote Jellyfish in plan 5's views.** A friend's or opponent's prefix 08 has no sprite on an updated badge. `battle_view` and `friends_view` belong to plan 5, which decides the fallback; this plan does not touch them.

**Owner answers (2026-11-14):** all five plan 7 questions are answered (see Global Constraints):
1. A saved Jellyfish moves to the Unicorn Realm with its record, then a new egg (T6, T8).
2. Old `PETS.CFG` keeps `8=Jellyfish`; the firmware hides pets without sprites (T6).
3. Plain white sky while the clock is unset (T1, T3).
4. Red cross for poorly (T1, T3).
5. The demo "Only pets hobby" moment goes to plan 8 as a scheduled demo timer (roadmap note in T8).

The §14 owner decisions (2026-11-13) and the brief's rules are the other inputs.

**Needs the owner:** nothing open.

**Type consistency:** checked `Sky`, `BackgroundState`, `pack`, `FLOOR_ITEMS`, `is_night`, `sky`, `floor_lines`, `floor_items`, `minutes_to_switch`, `background_state`, `wake_view`, `poorly`, `set_fake_unix`, `TEST_NOON_UNIX` / `SHOT_NOON_UNIX` (the same instant), `clock_sync_needs_redraw`, `draw_background`, `draw_backdrop`, `FLOOR_SLOTS`, `TUFT_XS`, `CROSS_BOX`, `SUN_MOON_BOX`, `NIGHT_LEVELS`, `debug_set_dirt_pct`, `scene_changes_per_day`, `drop_without_sprites`, `drop_from`, `is_hidden_kind`, `Retired`, `retire_hidden_pet`, `GameState::retire`, `RETIRED_LINES`, `load_changes`, `update_assets_to(.., changes)` and its `"no_space"` list, `volume_budget` and `PROTECTED` across the tasks that use them. `Visible` gains exactly one element (T2).

**Review Focus:** items 1 to 5 are pinned in T2, T3, T4, T2 / T8 and T5 / T6 / T7.

**Proportion:** 8 build tasks. The engine part (T1, T2) is small but carries the wake exactness. The drawing (T3) and the measurement (T4) carry the battery rule. The Jellyfish (T5 to T7) splits into tooling, firmware and assets, so a reviewer can reject one without the others.

**FAT12 budget** (1 MiB volume: 502 clusters of 2 KiB, 511 root entries):

| State | Files | Clusters |
|---|---|---|
| `assets/to-badge/` at `5e59574` | 422 | 505 (over by 3): PCX 490, MANIFEST 6, BPETS_V3.CFG 6, ALARMS 2, PETS.CFG 1 |
| Jellyfish removed (T7) | 372 | 429: PCX 415, MANIFEST 5 (9.3 KB), BPETS_V3.CFG 6 (about 11.6 KB with the 7 new keys, still under 12 KB), ALARMS 2, PETS.CFG 1 |
| New background or floor-item PCX | 0 | 0 (all drawn in code) |
| Free after this plan | 139 entries | 73 clusters (T7 test keeps at least 16 spare) |
| An updated old badge (`--update`) | | frees the 75 Jellyfish clusters when its files are the shipped ones; MANIFEST is replaced (6 to 5) |

**Flash:** no font, no image. Estimated 4 to 6 KiB in total, about 871 KiB of 960 after this plan, measured in Task 9:
- `background.rs`: dither loop, primitives and about 200 bytes of bitmaps, about 2.5 KiB;
- `scene.rs` and the solver lines: about 0.6 KiB;
- 7 `Thresholds` fields in two presets, their key strings and setters: about 0.6 KiB;
- the roster filter, the Realm retirement and the lifecycle hooks: about 1 KiB;
- the HAX backing and the two Gone-screen lines: under 0.2 KiB.

Lights Out stays.

**Refresh plan:** backgrounds are drawn into frames the screen already draws. They add 2 partial refreshes a day at the sky switch, plus at most one per floor-item step (at most 5 per dirt rise; a wash clears all in one), and no full refresh. Red appears only with the red line. The SSD1675 changed-pixel promotions from idle frames are measured and bounded at ≤ 24 a day (≤ 4 of them added by the night sky). The SSD1680 does no runtime full refresh.
