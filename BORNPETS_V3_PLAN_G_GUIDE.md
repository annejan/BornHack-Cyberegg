# BornPets v3, stage G: the player's guide Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** an A5, Amiga-booklet-style PDF guide to BornPets v3, built from `guide/` with one command. Every number in it comes from the code, every screen from the simulator, and every on-screen word is explained and indexed.

**Architecture:** one host-only bin, `guide` (`--features simulator`), writes four kinds of input into `guide/build/`: the numbers as JSON (Classic and Casual), the list of on-screen words as JSON, simulator screens as PNG, and pet sprites as PNG. Its logic lives in a `simulator`-gated library module, `src/guide/`, so integration tests can reach it. Typst reads those files. A missing number, screen or sprite fails the compile. `scripts/build_guide.py` runs the whole chain (cargo, bin, typst) and then checks the PDF: every on-screen word is indexed, every indexed word is on the badge, no typed digits in the prose, a picture on every page, the page count is a multiple of 4, and the colophon names the commit. `make guide` calls it. None of this builds into the firmware.

**Tech stack:** Rust (host `simulator` feature, embedded-graphics-simulator 0.8 for PNG output), Typst 0.15.1 (typst-cli), Python 3 stdlib.

**Spec:** `BORNPETS_V3_GUIDE_DESIGN.md` (owner-approved 2026-11-14). Roadmap row G: `BORNPETS_V3_ROADMAP.md`. Game rules: `BORNPETS_V3_DESIGN.md`. Where the design and the code differ, the code wins and the difference goes into the task report. The on-screen word list of plan 2 is in `BORNPETS_V3_PLAN_2_UI.md`, section "On-screen words".

**Code base:** worktree `BornHack-Cyberegg-v3guide`, branch `ranzbak/bornpets-v3-guide`, HEAD `64027d4`. Anchors at that commit:
- `src/bin/simulator.rs`: `TriColorDisplay` 13, `install_pets_from_repo_cfg` 66, `main` 76.
- `src/platform.rs`: `uptime_ms` 7.
- `src/game/engine/thresholds.rs`: `thresholds_table!` 32, `value_of` 44, `CLASSIC` 337 (simulator `HATCHING_TICKS: 1` at 345, Casual at 619), `Mode` 893, `BORNPETS_CFG_KEYS` 989, `mod template` 1648 (`BADGE_HATCHING_TICKS`, `cfg_template` 1660).
- `src/game/engine/needs.rs`: fixed lines 8 to 34. `status.rs`: `StatusWord` 26, `SECOND_WORD_UNDER` 124, `Word::ALL` 166, `Condition::ALL` 411. `causes.rs`: `Cause::all` 175. `hint.rs`: `subject_line` 69, `remedies` 111, `otherwise` 144, `Remedy::ask` 159. `doses.rs`: `Reason::unpack` 97, `price` 120, `apply` 212, `fall_asleep` 392. `debug.rs`: `DebugPoke` 15, `DebugInfo::line`, `debug_poke`.
- `src/game/engine/mod.rs`: `TICKS_PER_MINUTE` 48, HAX constants 116 to 143, `GameState` 239, `new_egg` 398 (`money: 100` 476), `catch_up` 520.
- `src/game/lifecycle.rs`: `now_tick` 42, `CATCH_UP_MAX_MIN` 198 (private), `start_new_game` 309, `cycle` 387, `onset_alert_due` 571.
- `src/game/mod.rs`: `Toast` 63, `Toast::message` 156, alerts 755 to 777, gone text 907 to 911, "No sprites on flash" 945.
- `src/game/modal.rs`: `ModalKind::title` 75, `items` 91, `DEBUG_ROWS` 236, `Item::label` 296, `shown_label` 895, `title_text` 905, hint title 1204.
- `src/game/pet_select.rs`: `MONEY_LABELS` 38, `NO_PETS_LINES` 46, prompts 203, 205, 269. `diary_view.rs`: `title` 57, "Nothing yet" 78. `stats_pages.rs`: `title` 37, `bars` 70. `diary.rs`: `DIARY_LEN` 11, `fmt_age` 55. `pacing.rs`: `IDLE_FRAME_SECS` 14.
- `src/game/sprite_loader.rs`: `count_anim_frames` 165, `blit_pcx_sim` 475. `engine/anim_files.rs`: `build_filename` 80.
- `Cargo.toml` `[[bin]]` list; `Makefile`; `flake.nix`; `.gitignore` (ignores `*.txt` and `docs`); `.github/workflows/tests.yml`.

## Global Constraints

Every task's requirements include this section.

**Owner rules (from the spec, verbatim where quoted):**
- Format "a PDF with graphics", style "Amiga-era game booklet", A5 portrait, page count a multiple of 4.
- Readers: "both the first-timer at the gate and the week-long carer". The first pages take a new player from "badge in hand" to "pet hatched, fed and asleep".
- "Every on-screen word gets explained somewhere", and every one is in the index with its page number.
- "Numbers come from the code and the presets, never from memory." Classic is the number in the prose. Casual gets the appendix column and a sidebar where it differs.
- Art: only repo assets (pet sprites, menu icons) and simulator screens, plus drawings made in Typst (bars, flags, diagrams). No AI art and no sponsor art (prefix 03 sprites, `sponsor-logos.png`, `bornpets-sprites-sponsers.png`).
- "If a page has no picture, the page is wrong."
- Tone: light and joking, "never at the expense of accuracy". Jokes never punch at people. The Hard mode (US) satire aims at the system (insurance paperwork, surprise bills, the copay, eviction) plus harmless clichés (giant portions, bald eagles, flags).
- "Alarms: the diabetes and dependence alarms fire again on every boot, unless the pet is hibernating. The guide says so."
- Labels: "If the labels in the game need to be changed to make the game flow more clear to the player, do so." Renames touch on-screen text only, never rules, save bytes, kv keys or config keys. Each one ships as its own small firmware commit with its tests.
- Unbuilt features stay out: no "coming soon" pages. Chapters 9 to 12 describe what the badge does today. A joke never implies a mechanic the badge lacks (today there are no deductibles, copays, bills or evictions).
- Public source (owner, 2026-11-14): "Public is fine". The whole guide source, the US chapter included, may go on the public GitHub mirror for CI. The owner still reads the PDF before anything is handed out.
- Chapters 9 to 12 (owner, 2026-11-14): write them now, in short form, describing today's badge only. No "coming soon" pages.
- Language: English, short sentences, plain words.

**Firmware safety:**
- The guide never changes firmware rules, saves, kv keys (`state`, `name`, `state_v3`, `name_v3`) or config keys, and never touches `BORNPETS.CFG` or the hand-added PCX files in `assets/to-badge/`.
- Every new Rust item is host-only: it lives in `src/guide/`, `src/sim_support.rs` or `src/bin/guide.rs` (all `simulator`-gated), or carries `#[cfg(any(test, feature = "simulator"))]`. The exceptions are visibility changes (`const` to `pub const`) and string literals moved into named `const`s with the same text. Both leave the firmware image unchanged.
- CI gates the SSD1680 release image at 960 KiB; it is about 853 KiB now. Task 16 reads the size from CI: it must not have grown by more than 1 KiB, apart from the label pass's text changes.

**Build machine:**
- Every local cargo command runs as `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo ... -j 2`. Any `build_guide.py` run that builds the bin uses the same prefix: `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py ...`. Only one cargo command runs at a time. Another stage builds on cores 0 and 1.
- **Test policy (owner):** each task runs only its own focused checks locally, and nothing is pushed per task. The full guide build (`make guide`: data, screens and PDF from scratch) runs once, in Task 16, which also pushes the branch so GitHub CI runs the full suite, both firmware builds and the flash gate. Task 17 fixes what Task 16 finds.
- Never use Codeberg CI. Every workflow job keeps `if: github.server_url == 'https://github.com'`. Push only to the `github` remote.
- Never use `git stash`: the stash is shared across worktrees. Do not run `cargo fmt`, because it rewrites unrelated files.
- Commit footer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

**Guide source rules (enforced by `scripts/build_guide.py`):**
- Chapter prose contains no digits. Numbers come from `v()`, `price()`, `until()`, `night()` and `dose()` (Task 2). Strings inside calls, `raw` spans and lines ending in `// digits-ok: <reason>` are exempt. Use the exemption only for facts that aren't game values.
- Every on-screen word in prose is written `#w("text")`, exactly as drawn. Placeholders are written in angle brackets (`#w("wait <time>")`).
- Cross-references use Typst labels (`@ch-care`), never typed chapter numbers.
- Generated content (data tables, the index) appears only in the appendix and the index. A chapter explains its words in hand-written prose or hand-written tables.

**Verify (V)**, the last step of every task: run the task's focused checks (listed in the task) once more on the committed tree, and paste their result lines into the report. No push and no CI per task; Task 16 does both.

**Partial guide builds**, the only guide builds a task before Task 16 runs:
- `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage <data|words|screens|sprites>` regenerates that one input in `guide/build/`.
- `python3 scripts/build_guide.py --pdf-only` compiles the PDF from the inputs already in `guide/build/` and runs every check. It needs no cargo. If an input it reads is missing, it exits 1 and names the `--stage` to run.

**Every chapter task's report** lists: (a) differences found between the code and `BORNPETS_V3_DESIGN.md` (the guide follows the code); (b) label-rename candidates, given as old → new → why, for any on-screen word that needs more than two sentences to explain. Task 14 applies the candidates the controller approves.

## Review Focus

These are the failure modes most likely to reach a reader unnoticed, most likely first. Each is pinned in the task named.

1. **Simulator values leak into the guide.** The `simulator` feature hatches in 1 tick, and the badge in 6. A dump or a screen built from the simulator preset would print the wrong hatching time. Pinned in Task 2 by `hatching_time_is_the_badges`, and in Task 4 by the `egg` shot's expectation (more than one tick left after the start).
2. **Words drift between the badge and the guide.** A rename or a new toast leaves the guide naming a word the badge no longer shows, or a word the badge shows that the guide never explains. Pinned in Task 3 by the two-way word check in `build_guide.py` (`test_marks_must_exist_on_the_badge`, `test_unmarked_word_fails`) and by `every_drawn_literal_is_listed`.
3. **A screen shows the wrong state.** After a firmware change a shot's steps no longer reach its state, and the PNG quietly shows something else. Pinned in Task 4 by each shot's `expect` predicate and by `every_shot_reaches_its_state`.
4. **A number is printed in the wrong unit.** For example, ticks read as minutes, or a per-mille rate shown as a percent. Pinned in Task 2 by `every_field_has_a_unit` and by `v()` refusing `pm` and `raw` values (a deliberate failing compile in Step 6).
5. **The colophon names a clean commit for a dirty tree.** The owner then compares a PDF to firmware it doesn't describe. Pinned in Task 1 by `test_colophon_must_match_git_describe` and by the dirty-tree step.

## File map

| File | Change |
|---|---|
| `flake.nix` | `typst` and `python3` in the dev shell |
| `.gitignore` | ignore `/guide/build/`, `/guide/out/`; un-ignore `guide/fonts/*.txt` |
| `Makefile` | `guide` target |
| `.github/workflows/tests.yml` | `guide` job (GitHub only) |
| `guide/typst-version` | create: the pinned Typst version, one line |
| `guide/main.typ` | create: the book, in chapter order |
| `guide/lib/booklet.typ` | create: page setup, styles, components, `w()`, `screen()`, `sprite()`, `finish()` |
| `guide/lib/data.typ` | create: number helpers over the JSON |
| `guide/lib/needs.typ` | create (Task 8): the nine need bars |
| `guide/chapters/NN-*.typ` | create: one file per chapter |
| `guide/fonts/` | create: Press Start 2P and its OFL licence |
| `scripts/build_guide.py`, `scripts/test_build_guide.py` | create: the build and its checks; their unit tests |
| `Cargo.toml` | `[[bin]] guide` |
| `src/lib.rs` | `pub mod guide;` and `pub mod sim_support;`, both `#[cfg(feature = "simulator")]` |
| `src/guide/{mod,json,data,words,shots,sprites}.rs` | create |
| `src/sim_support.rs` | create: display adapter, repo pet install, PNG save (moved from `simulator.rs`) |
| `src/bin/guide.rs` | create: the CLI |
| `src/bin/simulator.rs` | uses `sim_support`; F12 saves the screen |
| `src/platform.rs` | simulator-only fake clock |
| `src/game/engine/thresholds.rs` | host-only `FIELDS` and `badge_preset` |
| `src/game/lifecycle.rs` | `CATCH_UP_MAX_MIN` made `pub` |
| `src/game/{mod,modal,pet_select,diary_view}.rs`, `engine/status.rs`, `sprite_loader.rs` | host-only word, size and state accessors; literals moved to `const`s |
| `tests/guide_data.rs`, `tests/guide_data_casual.rs`, `tests/guide_words.rs`, `tests/guide_shots.rs` | create |
| `BORNPETS_V3_ROADMAP.md`, `README.md` | guide build line; "update the guide chapter" per plan |

---

### Task 1: Typst toolchain, guide skeleton and the one build command

**Files:**
- Modify: `flake.nix`, `.gitignore`, `Makefile`, `.github/workflows/tests.yml`, `README.md`.
- Create: `guide/typst-version`, `guide/main.typ`, `guide/lib/booklet.typ`, `guide/fonts/PressStart2P-Regular.ttf`, `guide/fonts/OFL.txt`, `scripts/build_guide.py`, `scripts/test_build_guide.py`.

**Interfaces:**
- Produces:
  - `guide/typst-version`: the line `0.15.1`.
  - `scripts/build_guide.py`: run from any directory; it `cd`s to `git rev-parse --show-toplevel`. Stages, in this order: `check_typst()`, `build_inputs(stages)` (empty in Task 1; Tasks 2 to 4 add the stages `data`, `words`, `screens` and `sprites`), `compile()`, `check()`. Flags: none = everything from a clean `guide/build/` (what `make guide` runs); `--stage <name>` (repeatable) = rebuild only those inputs, then compile and check; `--pdf-only` = no inputs rebuilt, only `compile()` and `check()`. `TYPST_ARGS = ["--root", ".", "--font-path", "guide/fonts", "--ignore-system-fonts", "--input", f"commit={commit}"]` is defined once and used by both `typst compile` and every `typst query`. Output: `guide/out/bornpets-guide.pdf`.
  - `commit = git describe --always --dirty=-dirty --abbrev=12`. Typst runs with `SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)`.
  - Check functions, pure so they can be unit-tested: `lint_digits(path: str, text: str) -> list[str]`, `check_pages(end_page: int) -> list[str]`, `check_colophon(found: str, expected: str) -> list[str]`. Each returns error strings, and `check()` exits 1 if any are returned.
  - `guide/lib/booklet.typ`: `booklet(body)` (show rule), `cover()`, `finish()`. `finish()` adds "Notes" pages so the total, back cover included, is a multiple of 4. It then places the back cover with the colophon, which emits `#metadata(sys.inputs.at("commit")) <colophon-commit>`, and ends with `#context [#metadata(here().page()) <guide-end>]`.
- Rules:
  - `check_typst()`: `typst --version` must report exactly the version in `guide/typst-version`. If it doesn't, exit 1 with: `guide: need typst <v>, found <x>. Install: CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo install typst-cli --locked --version <v> -j 2`.
  - `lint_digits`, per line of a `guide/chapters/*.typ` file:
    - skip lines that start (after spaces) with `//`, `#let`, `#import`, `#set`, `#show` or `#include`;
    - skip lines that end with `// digits-ok: ` followed by a reason;
    - remove string literals, `raw` spans (backticks), `<label>`s, `@ref`s, and the argument lists of `#name(...)` calls, balanced on that line;
    - any digit that is left is an error: `<file>:<line>: typed number in prose`.
  - `check_pages`: `end_page % 4 != 0` → `page count <n> is not a multiple of 4`.
  - `check_colophon`: the colophon commit must equal the build's `commit`.
- Page look: A5, a cream page (`#f3ead3`) with dark ink (`#2a2118`). Headings in Press Start 2P. Body text in Libertinus Serif, which is embedded in typst-cli, so no file is needed. On-screen words in DejaVu Sans Mono, also embedded. Chunky borders.

- [ ] **Step 1: install Typst locally.** nix is not on this machine, so use the fallback: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo install typst-cli --locked --version 0.15.1 -j 2`. Then `typst --version` must print `typst 0.15.1`.
- [ ] **Step 2: dev shell.** In `flake.nix`, add `typst` and `python3` to `packages`, with a comment saying that `scripts/build_guide.py` requires the version in `guide/typst-version`. This can't be checked here, because there is no nix: say so in the report.
- [ ] **Step 3: fonts.** Download `PressStart2P-Regular.ttf` and `OFL.txt` from `https://github.com/google/fonts/tree/main/ofl/pressstart2p` into `guide/fonts/`. If the download fails, stop and report BLOCKED. In `.gitignore`, add `/guide/build/`, `/guide/out/` and `!/guide/fonts/*.txt`.
- [ ] **Step 4: write the failing tests** in `scripts/test_build_guide.py` (unittest, stdlib, importing `build_guide` from the same directory):
```python
def test_digit_in_prose_fails(self):            # "Feed it 3 times." -> 1 error naming line 1
def test_digits_in_calls_strings_raw_pass(self): # '#v("FULL_LO_PCT") and #w("TYPE 2") and `BPETS_V3.CFG`' -> []
def test_code_lines_and_digits_ok_pass(self):    # "#let x = 4", "// 12", "A5 paper. // digits-ok: paper size" -> []
def test_page_count_multiple_of_four(self):      # check_pages(8) == [], check_pages(10) has 1 error
def test_colophon_must_match_git_describe(self): # check_colophon("abc-dirty", "abc") has 1 error; equal -> []
```
- [ ] **Step 5: run them and see them fail.** `python3 scripts/test_build_guide.py`: FAIL (no module).
- [ ] **Step 6: implement** `build_guide.py`, `booklet.typ` and `main.typ`. `main.typ` holds the cover, a one-paragraph placeholder "Contents" page with a sprite-free Typst drawing (a pixel heart from rects), and `finish()`. Add the Makefile targets `guide: ; python3 scripts/build_guide.py` and `guide-pdf: ; python3 scripts/build_guide.py --pdf-only` (and add both to `.PHONY`). In `README.md`, add one line under the build section: `make guide` builds the player's guide PDF into `guide/out/`.
- [ ] **Step 7: CI job.** In `.github/workflows/tests.yml`, add a job `guide` with `name: player's guide` and `if: github.server_url == 'https://github.com'`, running on `ubuntu-24.04`. Steps:
  1. checkout (submodules recursive);
  2. `sudo apt-get install -y libsdl2-dev libclang-dev`;
  3. `Swatinem/rust-cache@v2` with key `guide`;
  4. install Typst from the release binary: `v=$(cat guide/typst-version)`, then curl `https://github.com/typst/typst/releases/download/v$v/typst-x86_64-unknown-linux-musl.tar.xz`, verify it with `sha256sum -c` against a hash you pin in the workflow (compute it once from the download), and put `typst` on `PATH`;
  5. `python3 scripts/test_build_guide.py`;
  6. `CARGO_BUILD_JOBS=4 make guide`.

  Don't upload the PDF: the owner reads it before anything is handed out (see "Needs the owner").
- [ ] **Step 8: run them and see them pass.** `python3 scripts/test_build_guide.py`: PASS. `python3 scripts/build_guide.py --pdf-only` (no cargo yet in this task): exit 0, `guide/out/bornpets-guide.pdf` exists, and the check prints the page count (a multiple of 4).
- [ ] **Step 9: dirty-tree check.** Add a scratch line to `guide/main.typ` without committing it, run `python3 scripts/build_guide.py --pdf-only`, and confirm that `typst query ... "<colophon-commit>" --field value --one` ends in `-dirty`. Then revert the scratch line.
- [ ] **Step 10: commit** `build(guide): typst toolchain, booklet skeleton and make guide`.
- [ ] **Step 11: V.** Focused checks: `python3 scripts/test_build_guide.py` and `python3 scripts/build_guide.py --pdf-only`.

### Task 2: the numbers dump

**Files:**
- Modify: `src/game/engine/thresholds.rs` (macro 32 to 59, `mod template` 1648 to 1676), `src/game/lifecycle.rs` 198 (`pub const CATCH_UP_MAX_MIN`), `src/lib.rs`, `Cargo.toml`, `scripts/build_guide.py`.
- Create: `src/guide/mod.rs`, `src/guide/json.rs`, `src/guide/data.rs`, `src/bin/guide.rs`, `guide/lib/data.typ`.
- Test: `tests/guide_data.rs` (Classic), `tests/guide_data_casual.rs` (Casual). Each file is its own process, because `thresholds::install` runs once per process.

**Interfaces:**
- Produces (Rust):
```rust
// thresholds.rs, inside the macro, #[cfg(any(test, feature = "simulator"))]
impl Thresholds { pub const FIELDS: &'static [&'static str]; }      // every table field, table order
// thresholds.rs mod template (host-only), re-exported next to cfg_template
pub fn badge_preset(mode: Mode) -> Thresholds;   // mode.preset() with HATCHING_TICKS = BADGE_HATCHING_TICKS
// cfg_template() now reads badge_preset(Mode::Classic/Casual).value_of(key); its output is byte-identical
// src/guide/json.rs
pub enum J { Null, Int(i64), Str(String), Arr(Vec<J>), Obj(Vec<(String, J)>) }
impl J { pub fn render(&self) -> String; }       // insertion order; escapes " \ and control chars
// src/guide/data.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit { Pct, Min, Ticks, Secs, Count, Hax, Raw }
impl Unit { pub fn name(self) -> &'static str; }  // "pct" "min" "ticks" "secs" "count" "hax" "raw"
pub fn unit_of(field: &str) -> Unit;              // suffix rules + UNIT_OVERRIDES
pub fn fixed() -> Vec<(&'static str, u32, Unit)>;
pub fn prices(hard: bool) -> Vec<(&'static str, Option<u32>)>;
pub fn until() -> Vec<(&'static str, Option<u32>)>;   // minutes; needs the preset installed
pub fn night_min() -> Option<u32>;
pub fn doses() -> Vec<(&'static str, Result<Vec<(&'static str, i32)>, String>)>;
pub fn dump(mode: Mode) -> J;                     // installs badge_preset(mode) via sim::install_preset first
```
- Produces (CLI): `guide data --preset classic|casual --out <file>`.
- Produces (JSON), one file per preset at `guide/build/data/{classic,casual}.json`:
```
{ "preset": "Classic",
  "values": { "<KEY>": <u32>, ... },      // every Thresholds::FIELDS entry (badge values) and every fixed() entry
  "units":  { "<KEY>": "pct"|"min"|..., ... },
  "cfg_keys": [ "<KEY>", ... ],           // BORNPETS_CFG_KEYS order
  "prices": { "normal": { "<item>": <u32>|null }, "hard": { ... } },
  "until":  { "peckish": <min>|null, "thirsty": .., "sleepy": .., "bored": .., "grubby": .. },
  "night_min": <min>|null,
  "doses":  { "<item>": { "<field>": <tenths of a percent or minutes>, ... } | { "refused": "<reason text>" } } }
```
- Produces (Typst, `guide/lib/data.typ`): `classic`, `casual` (the parsed JSON, read from `/guide/build/data/`); `v(key, preset: classic)`, formatted by unit (`pct` → "40 %", `min` → "45 min", "6 h", "6 h 30 min" or "3 days", `ticks` → tick × 10 s formatted like `min` down to seconds, `secs` the same, `count` → the number, `hax` → "15 HAX"). `v()` panics for `raw` units (message: `use a derived number, not <key>`). `raw-v(key, preset)` → the bare number, for the appendix table only. `differs(key) -> bool` (Classic ≠ Casual). `price(item, hard: false)` → "15 HAX" or "free". `until(word)` and `night()` → a duration via the `min` formatter, panicking on `null`. `dose(item)` → the dictionary. A missing key fails the compile: always use `.at(key)`, never `.at(key, default: ..)`.

**Rules:**
- `unit_of` by suffix: `_PCT` → Pct; `_MIN` → Min; `_TICKS` and `_DURATION` → Ticks; `_PM`, `_DIV` and `_MULT` → Raw. `UNIT_OVERRIDES` gives the unit of every field with no suffix. The implementer reads each field's consumer to set it (for example `MINIGAME_COOLDOWN` and `BATTLE_COOLDOWN`, which are in ticks or minutes depending on where they're read; `MOOD_STEP`, `COLLAPSE_DAMAGE`, `MIN_TRAIT` and `MAX_TRAIT` → Raw; `DEPRIVED_NIGHTS`, `CLEARING_NIGHTS` and `HANGOVER_ASLEEP_MULT` → Count) and puts a one-line comment on each saying where the unit comes from.
- `fixed()`, each entry with its unit:
  - needs.rs: `NEED_DANGER_LO_PCT`, `NEED_HI_EDGE_PCT`, `ENERGY_HI_EDGE_PCT`, `DIRT_DANGER_PCT`, `HEALTH_DANGER_PCT`, `HUNGRY_PCT`, `EXHAUSTED_PCT`, `TIPSY_PCT`, `SORE_PCT`, `HAPPY_MOOD_PCT` (all Pct);
  - `SECOND_WORD_UNDER_PCT` = `status::SECOND_WORD_UNDER / P` (Pct);
  - `TICKS_PER_MINUTE`, `PET_NAME_MAX`, `REALM_MAX_PETS`, `DIARY_LEN`, `MEDICATION_HARD_MULT`, `REHAB_HARD_MULT` (Count);
  - `CATCH_UP_MAX_MIN` (Min);
  - `IDLE_FRAME_SECS` (Secs);
  - `MINIGAME_HAX_REWARD`, `BATTLE_HAX_REWARD`, `ONLYPETS_HOBBY_REWARD`, `PLAY_HAX_COST`, `DRUG_HAX_COST`, `ASPIRINE_HAX_COST`, and `START_HAX` = `GameState::new_egg(1, PetKind(0)).money` (Hax).
- `prices(hard)` comes from `doses::price` for `FoodKind::ALL` (key `label()`), `DrinkKind::ALL` (key `label()`) and the actions `Heal, Play, Medicate, Ozempic, Rehab, Exercise, Wash, Relax, OnlyPets` (key `format!("{a:?}")`).
- The reference pet: `GameState::new_egg(1, PetKind(0))` with `phase = Phase::Active`, `hatching_countdown = 0` and `is_sleeping = false` (as `hatched()` in `tests/engine_v3.rs` 832), then `debug_poke(DebugPoke::Comfort)`.
- `until()`: from the reference pet, step `catch_up(1)` up to 4320 times. Record the first minute each predicate holds, else `None`:
  - peckish: `fullness < needs::full_lo()`;
  - thirsty: `hydration < needs::water_lo()`;
  - sleepy: `energy < needs::energy_lo()`;
  - bored: `fun < needs::fun_lo()`;
  - grubby: `dirt > needs::dirt_hi()`.
- `night_min()`: from the reference pet, set energy to `energy_lo() - P`, call `doses::fall_asleep`, then step `catch_up(1)` until `!is_sleeping` (at most 1440 times).
- `doses()`: for each item, clone the reference pet, apply the setup poke, set `active_food` or `active_drink`, then call `doses::apply(&mut s, action)`. On `Err(r)`, record `Reason::Refused(r).text()`. Otherwise record the fields that changed:
  - the levels `full, water, energy, fun, dirt, booze, weight, fit, strain, mood, health` as `(after − before) × 10 / P` (tenths of a percent);
  - the timers `injured_min, hangover_min, groggy_min, insulin_min, rehab_min, ozempic_cd_min, med_cd_min` as their value after.

  Setups: foods → `Hungry`; drinks → `Thirsty`; `Wash` → `Dirty`; `Heal` → `Injured`; `Medicate` → `Diabetic`; `Ozempic` → `Fat`; `Rehab` → `Dependent`; `Play`, `Relax`, `Exercise` and `OnlyPets` get none.
- `values` must not have two entries with the same key; a test checks it.

- [ ] **Step 1: write the failing tests** in `tests/guide_data.rs` (Classic; it calls `bornhack_aegg::guide::data::dump(Mode::Classic)` and parses nothing, because the asserts use the Rust functions and the rendered string):
```rust
#[test] fn hatching_time_is_the_badges() { /* rendered JSON contains "\"HATCHING_TICKS\":6"; badge_preset(Classic).HATCHING_TICKS == 6 */ }
#[test] fn every_field_has_a_unit() { /* every Thresholds::FIELDS key is in "units"; unit_of never panics; no key in fixed() is a field */ }
#[test] fn values_match_the_presets() { /* for every field except HATCHING_TICKS: value == Thresholds::CLASSIC.value_of(k) */ }
#[test] fn cfg_keys_in_table_order() { /* cfg_keys == BORNPETS_CFG_KEYS names, same order */ }
#[test] fn prices_come_from_the_price_table() { /* Salad normal == price(Feed, Some(Salad), None, false); Salad hard > normal; Exercise == None */ }
#[test] fn the_reference_pet_gets_peckish_then_sleeps_and_wakes() { /* until() peckish is Some(m), 0 < m < 4320; night_min() is Some */ }
#[test] fn doses_report_effects_or_reasons() { /* Salad has "full" > 0; Wash has "dirt" < 0; Heal has "injured_min" */ }
#[test] fn json_escapes_quotes() { /* J::Str("a\"b").render() == "\"a\\\"b\"" */ }
```
`tests/guide_data_casual.rs`: `hatching_time_is_the_badges` (Casual) and `casual_values_match_the_preset`.
- [ ] **Step 2: run them and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test --features simulator --test guide_data -j 2`: FAIL (no module `guide`).
- [ ] **Step 3: implement** the thresholds additions and the `src/guide/` modules. In `lib.rs`, add `#[cfg(feature = "simulator")] pub mod guide;`. Add the bin `src/bin/guide.rs` with only the `data` subcommand. Unknown arguments exit 2 with a usage line. In `Cargo.toml`, add `[[bin]] name = "guide"`, `path = "src/bin/guide.rs"`, `required-features = ["simulator"]`.
- [ ] **Step 4: run them and see them pass,** then run the template tests that guard the refactor: `... --test guide_data`, `... --test guide_data_casual`, and `... --test engine_v3 template_`: PASS.
- [ ] **Step 5: wire the build.** In `build_inputs()`, the `data` stage builds the bin once with `cargo build --features simulator --bin guide -j <CARGO_BUILD_JOBS or 2>` and use `target/x86_64-unknown-linux-gnu/debug/guide` (check that the file exists and say so if the path differs). A full run deletes and recreates `guide/build/`; a `--stage` run recreates only its own subdirectory. The `data` stage runs `data` for both presets. Later stages reuse the same single `cargo build` call, which builds the bin once per run. Write `guide/lib/data.typ`. In `main.typ`, add one placeholder sentence that uses `#v("HATCHING_TICKS")`.
- [ ] **Step 6: prove the guards.** In a scratch chapter line, `#v("NO_SUCH_KEY")` must fail the compile, and so must `#v("FULL_DRAIN_AWAKE_PM")`. Remove the line. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage data`: exit 0, and the placeholder reads the badge's hatching time.
- [ ] **Step 7: commit** `feat(guide): host-only numbers dump read by the booklet`.
- [ ] **Step 8: V.** Focused checks: `... --test guide_data`, `... --test guide_data_casual`, `... --test engine_v3 template_`, and `python3 scripts/build_guide.py --pdf-only`.

### Task 3: the on-screen word list and the index marks

**Files:**
- Modify (host-only accessors, and literals moved into `const`s with identical text):
  - `src/game/modal.rs`;
  - `src/game/mod.rs` (alerts 755 to 777, gone text 907 to 911, 945);
  - `src/game/pet_select.rs` (203, 205, 269);
  - `src/game/diary_view.rs` (78);
  - `src/game/engine/status.rs` (`StatusWord::ALL`, host-only).
- Create: `src/guide/words.rs`. Modify: `src/bin/guide.rs` (`words`), `scripts/build_guide.py`, `scripts/test_build_guide.py`, `guide/lib/booklet.typ`, `guide/main.typ`.
- Create: `guide/covered-groups` (empty file for now; one group per line).
- Test: `tests/guide_words.rs`.

**Interfaces:**
- Produces (Rust, all `#[cfg(any(test, feature = "simulator"))]`):
```rust
// modal.rs: (group, text) for every menu title and every row shown in every ModalKind with the game's current
// money and hard modes (shown_label), every FoodKind::full_name(), and the templates "<price>H",
// "risk: <what>", "!" (row marker); hint group: "<name> says", "Your pet says", "but <reason>".
// Debug menu title and rows get group "debug".
pub fn menu_words() -> Vec<(&'static str, String)>;
// mod.rs: Toast::from_u8(0..=255) non-empty message() -> "toasts" (DebugCheat -> "debug"); alert lines and
// hints, the gone text and "No sprites on flash" -> "screen".
pub fn screen_words() -> Vec<(&'static str, &'static str)>;
// pet_select.rs
pub fn words() -> Vec<&'static str>;          // MONEY_LABELS, NO_PETS_LINES, the three prompts
// diary_view.rs
pub fn words() -> Vec<String>;                // title(0, 0), title(DIARY_LEN, 0), title(DIARY_LEN, 1), "Nothing yet"
// status.rs
impl StatusWord { pub const ALL: [StatusWord; 17]; }
// src/guide/words.rs
pub const GROUPS: [&str; 14] = ["status", "conditions", "causes", "reasons", "hint", "diary", "stats", "menu",
    "hard", "debug", "engine-info", "toasts", "screen", "pet-select"];
pub struct Entry { pub text: String, pub group: &'static str }
pub fn collect() -> Vec<Entry>;   // sorted by (group, text), no duplicate (group, text)
pub fn dump() -> J;               // { "groups": [...], "words": [ { "text": .., "group": .. } ] }
```
- Produces (CLI): `guide words --out <file>` → `guide/build/data/words.json`.
- Produces (Typst): `w(text)` in `booklet.typ` prints the word in the on-screen style (DejaVu Sans Mono on a white chip) and emits `#metadata(text) <gw>`.

**`collect()` sources by group:**
- status: `Word::ALL` `.text()`.
- conditions: `Condition::ALL` `.text()`, plus `+<n>`.
- causes: `Cause::all()` `.what()` and `.why()`.
- reasons: `Reason::unpack(k, 0).text()` for k in `0..=255`, deduplicated; kind 6 is emitted as `wait <time>`.
- hint:
  - `subject_line(s)` for `Subject::Word(w)` over `Word::ALL`, plus `WornOut`, `Egg`, `Gone` and `Hibernating`. A line equal to `format!("I'm {}!", w.text())` collapses into the single entry `I'm <word>!`.
  - `Remedy::ask` over the union of `remedies(s)`.
  - Non-empty `otherwise(s)`.
  - The modal hint entries.
- diary: `diary_view::words()`, plus `fmt_age(0)` and the templates `<n>m`, `<n>h`, `<n>d`.
- stats: `stats_pages::title` of both pages, and `bars(page, &stats).label` from a hatched pet's `stats()`.
- menu, hard, debug: call `menu_words()` three times, after `lifecycle::start_new_game(PetKind::roster()[0], money, hard)` with (false, false), (true, false) and (true, true). Labels seen only in the (true, true) run go in group `hard`. This needs pets installed: call `sim_support::install_pets_from_repo_cfg` if Task 4 has landed. Otherwise install a one-pet registry inline, and Task 4 replaces it.
- debug: the `DEBUG_ROWS` labels (through `menu_words`).
- engine-info: `StatusWord::ALL` `.name()`, plus `!`, `zz`, `nt`, `m<mood>/<target>`, `h<health>`, `w<wait>` and `w-` (the tokens of `DebugInfo::line`).
- toasts, screen: `screen_words()`.
- pet-select: `pet_select::words()`.

**Check rules (in `build_guide.py`; `words.json` is required from this task on):**
- `marked = typst query <TYPST_ARGS> guide/main.typ "<gw>" --field value`.
- `check_marks(marked: set[str], words: list[dict], groups: set[str]) -> list[str]`:
  - every marked text must be a word text (`marked word not on the badge: <t>`);
  - every word whose group is in `groups` must be marked (`unexplained <group> word: <t>`).
- `groups` = the lines of `guide/covered-groups` if the file exists, else every group in `words.json`.

- [ ] **Step 1: write the failing tests.** In `tests/guide_words.rs`:
```rust
#[test] fn plan_2_words_are_listed() {
    // texts contain: "peckish", "no insulin", "well rested", "worn out", "+<n>", "food coma", "ate too much",
    // "wait <time>", "too tired", "I'm <word>!", "Zzz...", "Please feed me.", "Diary 1/2", "Nothing yet",
    // "Needs 1/2", "Clean", "View stats", "Frikandel spec", "Frikandel speciaal", "risk: <what>", "Engine info",
    // "Hungover", "cheat applied", "Give rehab soon", "Hard mode (US)"
}
#[test] fn hamburger_is_hard_mode_only() { /* ("hard", "Hamburger") present; no ("menu", "Hamburger") */ }
#[test] fn groups_are_known_and_texts_non_empty() {}
#[test] fn every_drawn_literal_is_listed() {
    // for src/game/{mod,modal,pet_select,diary_view,stats_pages}.rs via include_str!: each string literal right after
    // `Text::with_text_style(` or `draw_centered_message(display, ` (whitespace allowed) is a collect() text
}
```
In `scripts/test_build_guide.py`:
```python
def test_marks_must_exist_on_the_badge(self):  # check_marks({"hungy"}, [{"text":"hungry","group":"status"}], set()) -> 1 error
def test_unmarked_word_fails(self):            # check_marks(set(), [{"text":"hungry","group":"status"}], {"status"}) -> 1 error
def test_uncovered_group_is_not_required(self): # same words, groups=set() -> []
```
- [ ] **Step 2: run them and see them fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test --features simulator --test guide_words -j 2` and `python3 scripts/test_build_guide.py`: FAIL.
- [ ] **Step 3: implement** the accessors, `words.rs`, the `words` subcommand, `w()`, the build step and `check_marks`. In the `main.typ` placeholder, mark `#w("hungry")`.
- [ ] **Step 4: run them and see them pass.** Both commands from Step 2: PASS. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage words`: exit 0.
- [ ] **Step 5: prove the guard.** Add `#w("hungy")` to the placeholder: `python3 scripts/build_guide.py --pdf-only` fails with `marked word not on the badge: hungy`. Remove it.
- [ ] **Step 6: commit** `feat(guide): on-screen word list and index marks, checked both ways`.
- [ ] **Step 7: V.** Focused checks: `... --test guide_words`, `python3 scripts/test_build_guide.py`, `python3 scripts/build_guide.py --pdf-only`. The firmware size is checked in Task 16.

### Task 4: simulator screens and pet sprites

**Files:**
- Create: `src/sim_support.rs`, `src/guide/shots.rs`, `src/guide/sprites.rs`. Modify: `src/lib.rs`, `src/bin/simulator.rs`, `src/bin/guide.rs`, `src/platform.rs` (7 to 20), `src/game/sprite_loader.rs` (host-only `pcx_size_sim`), `src/guide/words.rs` (use `sim_support`), `scripts/build_guide.py`, `guide/lib/booklet.typ`.
- Add host-only state accessors where a shot's `expect` needs one and none exists (for example `game::current_toast()` and `modal::hint_view_open()`), each `#[cfg(any(test, feature = "simulator"))]` and reading existing state only.
- Test: `tests/guide_shots.rs`.

**Interfaces:**
- Produces:
```rust
// platform.rs, #[cfg(feature = "simulator")]: uptime_ms() returns the fake value while it is set
pub fn set_fake_uptime_ms(ms: u64);                 // u64::MAX (the default) = use the real clock
pub fn fake_uptime_ms() -> Option<u64>;
// sim_support.rs (#[cfg(feature = "simulator")] in lib.rs); moved from simulator.rs, behaviour unchanged
pub struct TriColorDisplay<'a>(pub &'a mut SimulatorDisplay<Rgb888>);
pub fn install_pets_from_repo_cfg();
pub const PNG_SCALE: u32 = 4;
pub fn render() -> SimulatorDisplay<Rgb888>;       // 152 x 152, cleared white, draw_graphics(.., "sim", &85)
pub fn save_png(d: &SimulatorDisplay<Rgb888>, path: &Path) -> Result<(), String>;   // scale PNG_SCALE, spacing 0
pub fn non_white(d: &SimulatorDisplay<Rgb888>) -> usize;
// sprite_loader.rs, host-only
pub fn pcx_size_sim(name: &[u8; 11]) -> Option<(u32, u32)>;
// guide/shots.rs
pub const HERO_NAME: &str = "Bob";                  // builder's call; the hero is PetKind::roster()[0]
pub enum Step { Press(ButtonId), Start { money: bool, hard: bool }, Poke(DebugPoke), Skip(u32), Ticks(u32),
    Open(ModalKind), Do(fn()) }
pub struct Shot { pub name: &'static str, pub steps: &'static [Step], pub expect: fn() -> bool }
pub static SHOTS: &[Shot];
pub struct Summary { pub w: u32, pub h: u32, pub non_white: usize }
pub fn run(name: &str, out_dir: &Path) -> Result<Summary, String>;   // Err names the shot and the failed expect
// guide/sprites.rs
pub fn export(out_dir: &Path) -> J;                 // writes PNGs, returns the roster JSON
```
- Produces (CLI): `guide shots` (one name per line); `guide shot <name> --out <dir>` (writes `<dir>/<name>.png`, prints `<name> <w>x<h> non_white=<n>`, exits 1 on Err); `guide sprites --out <dir>` (PNGs and `<dir>/roster.json`).
- Produces (Typst): `screen(name, width: 46mm, caption: none)` shows `/guide/build/screens/<name>.png` with `scaling: "pixelated"` in a bezel frame. `sprite(pet, anim, frame: 0, width: auto)` shows `/guide/build/sprites/<prefix>-<anim>-<frame>.png`, where `pet` is a prefix or `"hero"`. `roster` is the parsed `roster.json`. A missing file fails the compile.

**Rules:**
- `run(name)`:
  1. `set_fake_uptime_ms(0)`;
  2. `sim::install_preset(thresholds::badge_preset(Mode::Classic), Mode::Classic, false)`, never the bare simulator preset;
  3. `install_pets_from_repo_cfg()`;
  4. run each step, then `lifecycle::cycle()`;
  5. evaluate `expect`;
  6. `render()` and `save_png`.

  One shot per process: the build script runs the bin once for each name.
- Step meanings:
  - `Start` calls `lifecycle::start_new_game(PetKind::roster()[0], money, hard)` and `set_pet_name(HERO_NAME.as_bytes())`;
  - `Ticks(n)` moves the fake clock forward `n × 10 000` ms;
  - `Skip(m)` calls `lifecycle::debug_skip_minutes(m)`;
  - `Poke(p)` calls `lifecycle::debug_poke(p)` (an `Err` fails the shot);
  - `Open(k)` calls `modal::open(k)`;
  - `Press(b)` calls `menu::dispatch_press(b)`.
- The first list of shots follows. The steps are a first guess. If an expectation fails, the implementer changes the steps (presses, skip minutes), never the expectation. A chapter task may add shots the same way.

| Shot | Steps | Expect |
|---|---|---|
| start | none | `!is_started()` |
| select-pet | Press Fire | `pet_select::is_active()` |
| select-mode | Press Fire ×2 | `pet_select::is_active()` (money phase) |
| naming | Press Fire ×3 | `text_entry::is_active()` |
| egg | Start (money) | anim is `Hatching { ticks_remaining }` with `ticks_remaining > 1` |
| hatched | Start, Ticks(hatch + 1) | started, not hatching |
| content | hatched, Poke Comfort | line first is `Content` |
| starving | content, Poke Hungry | first `Starving`, danger |
| two-words | content, Poke Hungry, Poke Bored | `second.is_some()` |
| asleep | content, Poke Tired, Do(sleep) | first `Asleep` |
| hung-over | content, Poke HungOver | conditions has `HungOver` |
| injured | content, Poke Injured | first `Injured` |
| wasted | content, Poke Drunk | first `Wasted`, danger |
| no-insulin | content, Poke Diabetic | line has `NoInsulin` |
| needs-rehab | content, Poke Dependent | line has `NeedsRehab` |
| stats-needs | starving, Open Stats, Press Fire | `stats_view_open()` |
| stats-body | stats-needs, Press Right | `stats_view_open()` |
| hint | starving, Open Stats, Press Down, Press Fire | hint view open |
| cause-toast | content, Do(feed Pizza), Ticks(feed + 1), repeated until a cause fires | current toast is `Cause` |
| diary | cause-toast, Open Stats, Press Down ×2, Press Fire | `diary_view::is_active()` |
| menu-feed / menu-drink / menu-rest / menu-heal / menu-play / menu-stats | content, Open the kind | `modal::is_open()` |
| menu-feed-hard | Start (money, hard), Ticks(hatch + 1), Poke Comfort, Open Feed | open and `hard_mode()` |
| risk-marker | content, Poke Stuffed, Open Feed | `modal::is_open()` |
| refusal-water | content, Open Drink, Press Fire | `reason_toast()` is `Some` |
| hibernating | content, Do(hibernate) | `is_hibernating()` |
| leaving | content, Poke Hungry, Poke Thirsty, Skip in steps until it holds | anim is `Leaving` |
| gone | leaving, Skip until it holds | anim is `Gone` |
| debug-menu | content, Press Up Up Down Down Left Right Left Right Fire | `modal::is_open()` |
| engine-info | debug-menu, Press Fire | current toast is `DebugInfo` |

- Sprites: for every `PetKind::roster()` entry, and for each `(name, DisplayAnim)` in:
  - `idle`, `happy`, `sleeping`, `feeding`, `drinking`, `playing`, `exercising`, `healing`;
  - `hungry` (WarningHungry), `tired` (WarningTired), `sick` (WarningSick), `critical` (CriticalSick);
  - `hibernating`, `egg` (`Hatching { ticks_remaining: 0 }`), `gone`;

  and each frame `0..frame_count(kind, anim)`, blit `build_filename(kind, anim, f)` at (0, 0) onto a white display of `pcx_size_sim` size. Save it at `PNG_SCALE` as `<prefix as 2 hex>-<name>-<f>.png`. Also save `000000.PCX` as `start.png`.

  `roster.json` = `{ "hero": { "name": HERO_NAME, "prefix": .. }, "pets": [ { "prefix": .., "name": .., "anims": { "<name>": <frame count> } } ], "start": "start.png" }`. Prefixes 03 and 04 never appear.
- `simulator.rs`: F12 saves `render()`-equivalent output of the live display to `guide/build/screens/manual-<n>.png` (n counts from 0 per run) and prints the path. Nothing else changes.

- [ ] **Step 1: write the failing test** `tests/guide_shots.rs`:
```rust
#[test] fn every_shot_reaches_its_state() {
    // names = stdout of Command::new(env!("CARGO_BIN_EXE_guide")).arg("shots"); at least 30
    // for each: `guide shot <name> --out <tempdir>` succeeds; stdout is "<name> 608x608 non_white=<n>" with n > 0
}
#[test] fn sprites_cover_the_roster_without_sponsors() {
    // `guide sprites --out <tempdir>`: roster.json lists every PETS.CFG prefix, none is 3 or 4; each pet has idle >= 1
}
```
- [ ] **Step 2: run it and see it fail.** `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test --features simulator --test guide_shots -j 2`: FAIL.
- [ ] **Step 3: implement** the fake clock, `sim_support` (and switch `simulator.rs` and `words.rs` to it), `pcx_size_sim`, the shots, the sprites and the CLI. Add the build steps that run every shot and then `sprites` into `guide/build/{screens,sprites}`. Add `screen()` and `sprite()`. In the `main.typ` placeholder, show `#screen("content")` and `#sprite("hero", "idle")`.
- [ ] **Step 4: run it and see it pass.** The command from Step 2: PASS. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage screens --stage sprites`: exit 0. Open five PNGs (`egg`, `starving`, `stats-body`, `risk-marker`, `engine-info`) and confirm that each shows what its name says. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo sim -j 2`, press F12 once: a PNG appears.
- [ ] **Step 5: commit** `feat(guide): simulator screens and sprites for the booklet`.
- [ ] **Step 6: V.** Focused checks: `... --test guide_shots` and `python3 scripts/build_guide.py --pdf-only`.

### Task 5: the booklet look: components, cover, "Meet the pets"

**Files:** Modify: `guide/lib/booklet.typ`, `guide/main.typ`, `scripts/build_guide.py`, `scripts/test_build_guide.py`. Create: `guide/chapters/00-meet-the-pets.typ`.

**Interfaces:**
- Produces (Typst, `booklet.typ`):
  - `pic()` emits `#context [#metadata(here().page()) <pic>]`. `screen`, `sprite`, `band-bar`, `pixel-flag` and `diagram-box` call it.
  - `tip(body)`, `joke(body)` and `warn(body)` are sidebar boxes with chunky borders, each with a small sprite or pixel icon.
  - `band-bar(zones, value: none)` draws a 0 to 100 bar. `zones` is an array of `(from, to, label, kind)`, with kind `"ok"`, `"warn"` or `"danger"` (cream, hatched, red). Labels sit under their zone, and an optional marker shows `value`.
  - `pixel-flag(width)` draws a US flag as a grid of square "pixels" (13 stripes, a canton of dots).
  - `diagram-box(body)` frames a hand-drawn diagram.
  - `hero` is `roster.hero.name`.
  - Chapter heading style: a level-1 heading starts a new page, shows the chapter number in the pixel font and a sprite of the hero, and sets a label from its file (`<ch-legend>` and so on).
- Check (in `build_guide.py`): `check_pictures(pic_pages: set[int], end_page: int) -> list[str]` returns `page <n> has no picture` for every page from 2 to `end_page - 1` missing from `pic_pages`. `pic_pages` comes from `typst query ... "<pic>" --field value`.

- [ ] **Step 1: write the failing test** `test_page_without_picture_fails` in `scripts/test_build_guide.py`: `check_pictures({2, 3}, 5) == []` and `check_pictures({2}, 5)` gives 1 error (page 3).
- [ ] **Step 2: run it and see it fail.** `python3 scripts/test_build_guide.py`: FAIL.
- [ ] **Step 3: implement** the components and `check_pictures`.
  - Cover: the title in the pixel font, the start screen sprite (`start.png`; if it shows sponsor art, use the roster idle sprites instead and say so), and the line "The official player's guide".
  - `00-meet-the-pets.typ`: one spread with every roster pet (idle and happy sprites, name), the hero named and marked as the guide's walking example, and one `joke()` box. No numbers.
  - Remove the Task 1 to 4 placeholder lines from `main.typ`.
- [ ] **Step 4: run it and see it pass.** `python3 scripts/test_build_guide.py`: PASS. Then `python3 scripts/build_guide.py --pdf-only`: exit 0, with no page lacking a picture. Look at the PDF: the sprites are crisp, not smoothed.
- [ ] **Step 5: commit** `feat(guide): booklet components, cover and the pet roster spread`.
- [ ] **Step 6: V.** Focused checks: `python3 scripts/test_build_guide.py` and `python3 scripts/build_guide.py --pdf-only`.

### Chapter tasks (6 to 13): common steps

Each chapter task creates its chapter files, includes them in `main.typ` in book order, and follows these steps. "Must mark" lists the word groups the chapter completes. At the end, the task appends those groups to `guide/covered-groups`, so from then on the check requires every word in them to be marked.

- [ ] **Step 1: draft** the chapter from the outline and the sources given. Read the code, not the design, for every rule. Each page gets a screen, a sprite or a drawing.
- [ ] **Step 2: numbers.** Every number goes through `v()`, `price()`, `until()`, `night()` or `dose()`. Where `differs(key)` is true, add a "Casual:" line in a `tip()`.
- [ ] **Step 3: words.** Mark every on-screen word with `#w()`. Append the "Must mark" groups to `guide/covered-groups`.
- [ ] **Step 4: build.** `python3 scripts/build_guide.py --pdf-only`: exit 0. If the chapter added a shot, run `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage screens` first, and also `... --test guide_shots`. That means no typed digits, every marked word is on the badge, the covered groups are fully marked, there is a picture on every page, and the page count is a multiple of 4.
- [ ] **Step 5: read it** in the PDF once, next to the screens. If a rule's description and the code disagree, the code wins.
- [ ] **Step 6: commit** `docs(guide): <chapter titles>` (the guide only; no firmware change in a chapter task).
- [ ] **Step 7: V.** Focused check: `python3 scripts/build_guide.py --pdf-only` (plus `--test guide_shots` if a shot was added). The report gives the design differences and the rename candidates (see Global Constraints).

### Task 6: chapters 1 and 2: the legend, getting started

**Files:** Create: `guide/chapters/01-legend.typ`, `guide/chapters/02-getting-started.typ`. Modify: `guide/main.typ`, `guide/covered-groups`.
**Must mark:** `pet-select`.

- **The legend (one page):** a short story about why pets live on badges at BornHack, and what the Unicorn Realm is (where pets go when they leave; detail in chapter 12). Art: the egg and hero sprites. Sources: `README.md` intro, `src/game/realm_view.rs`.
- **Getting started**, written as a numbered "first ten minutes" walk-through that ends with the pet hatched, fed and asleep (Goal 1):
  - the buttons (`ButtonId`: the joystick's Up, Down, Left and Right, plus Fire, Cancel and Execute; physical layout from `QUICKSTART.md`);
  - the game screen, and moving between screens with Left and Right (`menu.rs` `ScreenId`);
  - starting a game, choosing a pet, the three money modes with one line each, naming (`v("PET_NAME_MAX")` characters; a random Danish or Dutch default), hatching (`v("HATCHING_TICKS")`);
  - the first feed, the first drink, putting the pet to bed (pointers to `@ch-care`);
  - Classic or Casual, one line pointing to `@ch-hackers`;
  - what `No pets installed`, `Check PETS.CFG` and `No sprites on flash` mean.
- Sources: `src/game/input.rs` (whole file), `pet_select.rs`, `lifecycle.rs` `start_new_game` 309 and `set_pet_name` 337, `text_entry.rs`, `menu.rs` 2047 (mode toggle).
- Screens: `start`, `select-pet`, `select-mode`, `naming`, `egg`, `hatched`, `menu-feed`, `menu-rest`, `asleep`.

### Task 7: chapter 3: reading your pet

**Files:** Create: `guide/chapters/03-reading.typ`. **Must mark:** `status`, `hint`, `diary`.

- The screen layout as an annotated screen: the pet area, the status line, the toast corner, HAX at the top right in money modes, and the icon grid.
- The status line:
  - the first word, the second word (shown only when its pull is at least `v("SECOND_WORD_UNDER_PCT")`), the prompts (`no insulin`, `needs rehab`), and the info words when nothing is wrong;
  - red means danger, and the order danger words are picked in;
  - a hand-written table of all status words, each with one line and the chapter that covers it.
- The buzz: only for the alerting words and danger, only when things get worse, and it can be muted.
- The idle frame: one frame per `v("IDLE_FRAME_SECS")`. Actions animate at full speed. Why e-paper sleeps.
- The Hint row: how to read `<name> says`, the `I'm <word>!` pattern, every ask, every fixed line, and `but <reason>` (the pet never asks for something the menu refuses).
- The Diary: the last `v("DIARY_LEN")` causes, two pages, ages, cleared on reboot.
- Sources: `status.rs` (`status_line`, `danger_word`, `Word::alerts`), `hint.rs`, `diary.rs`, `diary_view.rs`, `pacing.rs`, `lifecycle.rs` `check_severity_transition`, `mod.rs` status line drawing, plan 2 Tasks 1, 2, 3, 5 and 9.
- Screens: `content`, `two-words`, `starving`, `asleep`, `wasted`, `hint`, `cause-toast`, `diary`.

### Task 8: chapter 4: the nine needs

**Files:** Create: `guide/chapters/04-needs.typ`, `guide/lib/needs.typ`. **Must mark:** `stats`.

- `guide/lib/needs.typ`: `need-bar(name)` for each of `full`, `water`, `energy`, `fun`, `dirt`, `booze`, `weight`, `fitness` and `strain`. It builds `band-bar` zones from `v()` values only, with the band edges from `thresholds.rs`, the fixed lines from `needs.rs`, and the graded words from `status.rs`. Examples:
  - full: `starving` below `NEED_DANGER_LO_PCT`, `hungry` below `HUNGRY_PCT`, `peckish` below `FULL_LO_PCT`, comfort up to `FULL_HI_PCT`, `stuffed` above;
  - booze: `tipsy` from `TIPSY_PCT`, `sloppy` above `BOOZE_HI_PCT`, `wasted` above `BOOZE_DANGER_PCT`;
  - strain: `sore` above `SORE_PCT`, injury at `STRAIN_INJ_PCT`.

  Each zone is read from the code that names it.
- One half-spread per need, holding:
  - the bar;
  - what pushes the need up and down, in words, with `until()` times and `dose()` effects where they exist;
  - what happens past each edge (the mood penalty, health damage past the damage line, danger);
  - one `joke()` or `tip()`.

  Watch for these: "Clean" on the stats page is the reverse of dirt; energy is a floor, not a band; and weight counts both ways.
- The stats view: two pages, the labels, the band ticks, red when out of band.
- Sources: `needs.rs`, `regime.rs` (`rates`, `penalty_terms`, `health_damage`), `status.rs` grading, `stats_pages.rs`, `stat_bar.rs`; design 2.1, 2.2 and 15.1 to 15.3 for intent only.
- Screens: `stats-needs`, `stats-body`, `starving`, `wasted`, `injured`.

### Task 9: chapter 5: care actions

**Files:** Create: `guide/chapters/05-care.typ`. **Must mark:** `menu`, `reasons`, `toasts`.

- One section per menu, in icon order. Each covers: what each row does (from `dose()`, in words with numbers), what it costs (`price()`), how long it takes (the `*_DURATION` keys), what it risks (the ` !` marker and `risk: <what>`), and every reason it can be refused (`doses::refusal`, `modal.rs` `available` and `reason`).
  - Feed: the five foods with their full names; the repeat-junk rule (`DOSE_JUNK_*`).
  - Drink: water, cola and the three alcohols (booze rules come in `@ch-conditions`).
  - Rest: Sleep (`night()`), Relax, Wash, Wake up.
  - Heal: Aspirine, Insulin, Ozempic, Rehab.
  - Play: Battle (one line, then `@ch-friends`), Play now, Only pets (money modes), Play music, and the five mini-games (`MINIGAME_COOLDOWN`, the `DOSE_MINI_*` grades).
  - Exercise.
  - Stats: View stats, Hint, Diary, Rolled stats, Friends.
  - Hibernate and Defrost (then `@ch-offtime`).
  - Music: the songs, one joke line.
- Cooldowns show as `wait <time>`. Prices show as `<price>H`.
- A hand-written table of every toast, saying when it appears and what it means. Station and battle toasts get one line each, plus a cross-reference.
- Sources: `modal.rs` (`items` 91, `available`, `reason`, `activate`, `row_text`, `title_text`), `doses.rs`, engine `mod.rs` starters 722 to 840, `minigame.rs`, `mod.rs` `Toast::message`.
- Screens: `menu-feed`, `menu-drink`, `menu-rest`, `menu-heal`, `menu-play`, `menu-stats`, `risk-marker`, `refusal-water`.

### Task 10: chapters 6 and 7: mood and health, conditions

**Files:** Create: `guide/chapters/06-mood-health.typ`, `guide/chapters/07-conditions.typ`. **Must mark:** `conditions`, `causes`, `screen`.

- **Mood and health:**
  - mood moves toward a target set by the needs (`regime::mood_target`, `MOOD_STEP`);
  - the health ceiling from fitness (the `CEILING_*` keys) and the diabetic ceiling;
  - damage (the `DMG_*` lines) and recovery (the `REC_*` keys);
  - collapse (`COLLAPSE_*`), leaving (`LEAVING_MIN`) and Gone, with its screen text.

  Art: a Typst `diagram-box` of "needs → mood target → mood → health" (design 9.1 for the shape, the code for the arrows), plus the `leaving` and `gone` screens.
- **Conditions**, one section each, saying how the pet gets it, what it does, and how to get rid of it:
  - hangover (`HANGOVER_MIN`, water shortens it, longer asleep);
  - groggy (oversleeping, woken too early, `GRUMPY_*`);
  - worn out (`DEPRIVED_NIGHTS`, `NIGHT_SHORT_MIN`, `CLEARING_NIGHTS`, the energy cap);
  - sore and injury (`SORE_PCT`, `STRAIN_INJ_PCT`, `DOSE_INJURY_MIN`, Aspirine shortens it);
  - dependence and rehab (onset, relapse, cravings, `DOSE_REHAB_MIN`);
  - diabetes and its reversal (onset weight band, insulin window, reversal fitness and time);
  - well rested.
- The boot alarms, said plainly: the diabetes and dependence alarms fire again on every boot unless the pet is hibernating, and the full-screen alerts and their lines.
- A hand-written table of every cause toast (what, why, what to do).
- Sources: `regime.rs` 188 to 354, `advance.rs` (whole file), `doses.rs`, `causes.rs`, `status.rs` `conditions`, `lifecycle.rs` 560 to 620, `mod.rs` alerts 755 to 777 and gone text 907.
- Screens: `hung-over`, `injured`, `no-insulin`, `needs-rehab`, `stats-body`, `cause-toast`. Add a shot `alert-diabetes` with expect "the diabetes alert is active", which needs a host-only accessor.

### Task 11: chapters 8, 9, 11 and 12: off-time, money, friends and battles, the Unicorn Realm

**Files:** Create: `guide/chapters/08-offtime.typ`, `09-money.typ`, `11-friends.typ`, `12-realm.typ`. **Must mark:** none new. Each chapter describes what the badge does today, with no promises.

- **Off-time and hibernation:**
  - no time passes while the badge is off, until the clock is known;
  - after that, the catch-up on boot is at most `v("CATCH_UP_MAX_MIN")`;
  - how to set the clock (pointer to `USER_WATCH.md`);
  - what hibernation freezes, Defrost, and that alarms stay quiet while hibernating.

  Sources: `lifecycle.rs` 185 to 320 (`catch_up_minutes`, `deferred_catch_up_minutes`, `on_wall_clock_set`), engine hibernation. Screen: `hibernating`.
- **Money and HAX:**
  - the starting HAX (`v("START_HAX")`);
  - a hand-written price table using `price()` for every food, drink and action, in both money modes (chapter 10 adds the hard column);
  - earning by Only pets (`ONLYPETS_HOBBY_REWARD` and its dose), mini-game wins (`MINIGAME_HAX_REWARD`) and battles (`BATTLE_HAX_REWARD`);
  - `can't afford`;
  - playing without money.

  Sources: `doses::price`, engine `mod.rs` 116 to 143, `lifecycle::award_inspiration`, `battle.rs`.
- **Friends, battles and stations:**
  - friends met over the mesh, and the friend boost (`DOSE_FRIEND_*`);
  - battles as they work today (`battle.rs`, `BATTLE_COOLDOWN`, the picker title `Battle <w>W-<l>L`);
  - NFC stations, their phrases (`station.rs` 78 to 80, written as raw text), and their toasts and cooldown.

  Sources: `friends.rs`, `battle.rs`, `battle_view.rs`, `station.rs`, `USER_NFC.md`, `NFC_README.md`.
- **The Unicorn Realm:** past pets (`v("REALM_MAX_PETS")`), generations, a new egg after Gone. Sources: `realm_view.rs`, `lifecycle::new_generation` 864, `PetRecord`. Art: a row of sprites.

### Task 12: chapter 10: Hard mode (US)

**Files:** Create: `guide/chapters/10-hard-mode.typ`. **Must mark:** `hard`.

- What the badge does today:
  - healthy food costs more (both price columns from `price(.., hard: true)`);
  - medication costs ×`v("MEDICATION_HARD_MULT")` and rehab ×`v("REHAB_HARD_MULT")`;
  - the Frikandel becomes a `Hamburger`;
  - how to pick `Hard mode (US)`.
- Style: `pixel-flag()` in the margins, giant-portion and bald-eagle jokes, and satire of insurance paperwork and the system in `joke()` boxes.
- Rules for this chapter:
  - no joke at people or at a nationality's people;
  - no joke that implies a mechanic the badge lacks today (no deductibles, copays, bills, authorization popups or eviction: plan 6 brings those, and this chapter grows then);
  - the owner reads this chapter against the badge before the guide is handed out.
- Sources: `doses.rs` `price` 120, engine `mod.rs` 133 to 143, `modal.rs` `shown_label` 895, `pet_select.rs` 38 and 165.
- Screen: `menu-feed-hard`.

### Task 13: chapters 13 and 14: secrets, for hackers

**Files:** Create: `guide/chapters/13-secrets.typ`, `guide/chapters/14-hackers.typ`. **Must mark:** `debug`, `engine-info`.

- **Secrets (a hidden page;** the visual trick is the builder's call): the button sequence (`debug_cheats.rs` 17 to 27, written as arrows), every debug row and what it does (`DEBUG_ROWS`, `DebugPoke` docs), `cheat applied`, and the Engine info line token by token (`DebugInfo::line`: the word, `!`, `zz`, `nt`, mood/target, health, wait, `w-`). Screens: `debug-menu`, `engine-info`.
- **For hackers (appendix):**
  - `BPETS_V3.CFG`: where it lives, `KEY=VALUE`, "uncomment a line", unknown keys ignored, values clamped, a bad band pair falling back to the preset (`validate`), a reboot to apply, and the `*` next to the name for custom values;
  - Classic and Casual, and where to switch;
  - `PETS.CFG` and the roster;
  - going back to the old firmware: it keeps `BORNPETS.CFG`, the `state` save and the name; v3 saves under `state_v3` and `name_v3`;
  - pointers to `USER_GAMES.md`, `README.md` and `QUICKSTART.md`.
  - A generated table, the one allowed outside the index: every `cfg_keys` entry with its unit, Classic and Casual, using `raw-v`, with Casual in bold where `differs`.

  Sources: `thresholds.rs` 989 and 1615, `bornpets_cfg.rs`, `pets_cfg.rs`, engine `mod.rs` 1608 to 2083, `settings.rs`.

### Task 14: label pass

The controller collects the rename candidates from the reports of Tasks 6 to 13 and approves a list. If the list is empty, this task only records that in its report.

**Files:** per rename: the one source that holds the text (for example `status.rs` `Word::text`, `causes.rs`, `doses.rs` `Refusal::text`, `modal.rs` `Item::label`, `mod.rs` `Toast::message`), the tests that assert the old text, the plan 2 width tests, and the guide chapters that `#w()` it.

**Width budgets** (from plan 2; FONT_7X13 is 7 px per column, and both panels are 152 x 152):

| Where | Budget |
|---|---|
| Toast line | ≤ 15 columns, at most 2 lines |
| Status line | ≤ 21 columns per line, at most 2 lines |
| Popover body and title | ≤ 18 columns |
| Menu row (price and marker included) | ≤ 17 columns |
| Full-screen view line | ≤ 21 columns |
| Stats bar label | ≤ 8 columns |

For each approved rename, in its own commit:
- [ ] **Step 1:** change the string at its one source. Never touch a rule, a save byte, a kv key or a config key.
- [ ] **Step 2:** update the tests that assert the old text, and run them with the focused command for their file (for example `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test --features simulator --lib game::modal -j 2` or `... --test engine_v3 status_line`). The worst-case width tests must still pass.
- [ ] **Step 3:** `CARGO_BUILD_JOBS=2 taskset -c 2,3 python3 scripts/build_guide.py --stage words` now fails with `marked word not on the badge: <old>`. Change the guide's `#w("<old>")` to the new text and rebuild: exit 0.
- [ ] **Step 4: commit** `fix(game): rename "<old>" to "<new>" for clarity`, with a body that says why.
- [ ] **Step 5: V** after the last rename: the focused tests of every file touched, `... --test guide_words`, and `python3 scripts/build_guide.py --pdf-only`. The report gives the list as old → new → why, for the owner.

### Task 15: the index, full coverage, roadmap

**Files:** Create: `guide/chapters/15-index.typ`. Modify: `guide/main.typ`, `scripts/build_guide.py`, `scripts/test_build_guide.py`, `BORNPETS_V3_ROADMAP.md`. Delete: `guide/covered-groups`.

- The index: in a context block, `query(<gw>)` gives each word once, sorted without regard to case, with its pages (deduplicated and ascending), set in two columns. It carries small sprites as section dividers (A, B, ...), so its pages pass the picture check.
- Full coverage: delete `guide/covered-groups`, and remove from `build_guide.py` the code that reads it (no dead code). The check then always requires every group. Update `test_uncovered_group_is_not_required` to match: delete it, and add `test_every_group_is_required`.

- [ ] **Step 1: change the test** as above. `python3 scripts/test_build_guide.py`: FAIL.
- [ ] **Step 2: implement** the index and the coverage change.
- [ ] **Step 3: build.** `python3 scripts/test_build_guide.py`: PASS. `python3 scripts/build_guide.py --pdf-only`: exit 0, with every on-screen word marked.
- [ ] **Step 4: roadmap.** In `BORNPETS_V3_ROADMAP.md`, add to the "Ships" cell of each row: plan 3 "updates guide ch 3 and 5", plan 4 "updates guide ch 11", plan 5 "updates guide ch 11", plan 6 "updates guide ch 9 and 10", plan 6b "updates guide ch 10", plan 7 "updates guide ch 3", plan 8 "updates guide ch 12". Add one sentence under the table: every later plan runs `make guide` and fixes what the check flags.
- [ ] **Step 5: commit** `docs(guide): index of on-screen words; every word required`.
- [ ] **Step 6: V.** Focused checks: `python3 scripts/test_build_guide.py` and `python3 scripts/build_guide.py --pdf-only`.

### Task 16: regression and integration

This is the first push and the only full guide build of the stage. Change no code here: write down every failure for Task 17.

**Files:** none changed. The output is the report.

- [ ] **Step 1: full guide build from scratch.** `rm -rf guide/build guide/out`, then `CARGO_BUILD_JOBS=2 taskset -c 2,3 make guide`. Expected: exit 0. That means:
  - data for both presets, every word, every shot and every sprite are regenerated;
  - the PDF compiles, which proves no number key, screen or sprite is missing;
  - every check passes: every on-screen word marked, every mark on the badge, no typed digits, a picture on every page, a multiple of 4 pages, and the colophon equal to `git describe`.

  Record the page count and the colophon commit.
- [ ] **Step 2: guard spot-checks** (scratch edits, reverted after each one; none may be committed). Each must fail `python3 scripts/build_guide.py --pdf-only` with the named message:
  - `#v("NO_SUCH_KEY")` → a missing key;
  - `#v("FULL_DRAIN_AWAKE_PM")` → `use a derived number`;
  - `#w("hungy")` → `marked word not on the badge: hungy`;
  - removing the only `#w("peckish")` → `unexplained status word: peckish`;
  - typing `Feed it 3 times.` in a chapter → `typed number in prose`.

  Afterwards, `git status --short` must show no change under `guide/` or `src/`.
- [ ] **Step 3: firmware unaffected locally.** In `src/lib.rs`, `pub mod guide;` and `pub mod sim_support;` are both behind `#[cfg(feature = "simulator")]`. Then `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo check --bin embassy --no-default-features --features embassy-1680-full --target thumbv7em-none-eabihf -j 2`: exit 0.
- [ ] **Step 4: push and run the full CI.**
  1. `SSH_AUTH_SOCK=/run/user/1000/gcr/ssh git push github ranzbak/bornpets-v3-guide`.
  2. `gh run list -R ranzbak/bornhack-firmware-2026-ce -b ranzbak/bornpets-v3-guide -L 1 --json databaseId,headSha`. The `headSha` must equal `git rev-parse HEAD`.
  3. `gh run watch <id> -R ranzbak/bornhack-firmware-2026-ce --exit-status`.
  4. `gh run view <id> -R ranzbak/bornhack-firmware-2026-ce --json conclusion,jobs`.

  Expected: every job succeeds. That covers the six `test` suites (including `tests/guide_*.rs` through "others"), the `firmware build` (both panels and the 960 KiB gate) and `player's guide`. Record the SSD1680 release size: it must be within 1 KiB of 853 KiB, plus what the label pass's text changes added.
- [ ] **Step 5: report.** List every failure: the job, the test or check, the message, and the task that owns the code. Also give the PDF path, the page count, the colophon commit, the SSD1680 size, all the design differences from Tasks 6 to 13, and the renames from Task 14. If there were no failures, say so: Task 17 then only runs its Steps 5 and 6.

### Task 17: bug-fix round

**Files:** whatever the Task 16 report names, and nothing else.

For each failure in the Task 16 report, in report order:
- [ ] **Step 1: reproduce it locally** with the narrowest command. That is one of:
  - the failing test by name: `CARGO_BUILD_JOBS=2 taskset -c 2,3 cargo test --features simulator --test <file> <name> -j 2`;
  - `python3 scripts/test_build_guide.py`;
  - `python3 scripts/build_guide.py --pdf-only` or `--stage <name>`.

  A CI-only failure (firmware build or flash gate) is reproduced with the same `cargo build` line as the workflow, with the core prefix.
- [ ] **Step 2: fix it** in the owning task's scope. Never weaken a check, an expectation or a test to make it pass. Never change a firmware rule, save, kv key or config key.
- [ ] **Step 3: run the Step 1 command again:** PASS.
- [ ] **Step 4: commit** `fix(guide): <what>` (or `fix(game): ...` for firmware text), one commit per failure.

Then, once:
- [ ] **Step 5: confirm.** `rm -rf guide/build guide/out && CARGO_BUILD_JOBS=2 taskset -c 2,3 make guide`: exit 0. Push and watch CI as in Task 16 Step 4: every job succeeds. If something new fails, go back to Step 1 for it.
- [ ] **Step 6: final report** for the owner's read against the badge: the PDF path, the page count, the colophon commit, the SSD1680 size, the fixes made, the design differences, and the renames.

---

## Self-review

**Spec coverage:**

| Spec item | Where |
|---|---|
| PDF with graphics, Amiga booklet, A5, multiple of 4 | T1 (setup, `finish`, page check), T5 (look) |
| Both readers; first pages carry a new player to a sleeping pet | T6 walk-through |
| Every on-screen word explained and indexed | T3 (list and marks, both ways), T6 to T13 (groups), T15 (index, full coverage) |
| Numbers from the code; Classic in prose, Casual column and sidebar | T2 (dump, helpers, units), chapter Step 2, T13 appendix |
| Colophon commit hash | T1 |
| Pixel art: real sprites, real screens, band drawings | T4, T5, T8 |
| Contents 1 to 15 | T6 (1, 2), T7 (3), T8 (4), T9 (5), T10 (6, 7), T11 (8, 9, 11, 12), T12 (10), T13 (13, 14), T15 (15); "Meet the pets" T5 |
| Hard mode (US) with flags and stereotypes, satire at the system | T12, Global Constraints |
| Alarms fire again on every boot unless hibernating | T10 |
| Label pass, separate small commits with tests | chapter reports, T14 |
| Unbuilt features left out, no "coming soon" | Global Constraints, T11, T12 |
| Art only from the repo; no sponsor or AI art | Global Constraints, T4 (no prefix 03 or 04), T5 cover check |
| Typst (owner's choice), in `flake.nix`, built on 2 cores | T1 |
| Screen dump simulator-only; script drives the debug pokes | T4 |
| One command rebuilds data, screens and PDF | T1 to T4 (`make guide`), first full run T16 |
| Owner test policy: focused tests per task, one regression run, one fix round | Global Constraints, V in every task, T16, T17 |
| `guide/` with sources, scripts, fonts and licences; PDF in `guide/out/` | T1 |
| Roadmap row G (exists) and an "update the guide chapter" line per later plan | T15 |
| Not a replacement for README, QUICKSTART, USER_GAMES; points to them | T6, T13 |
| Owner's read is the acceptance gate | T17 final report; Needs the owner |

**Builder's calls, stated:**
- One `guide` bin with subcommands, and a `simulator`-gated lib module, so the integration tests reach it and CI's "others" loop runs `tests/guide_*.rs` with no workflow change.
- The screens come from a headless driver with a fake clock, not from scripting the SDL window. The interactive simulator gains F12 for manual screens. Both share `sim_support`.
- The checks live in Python next to the build, because `typst query` gives the marks, picture pages and colophon.
- Words come from the code (enumerations and accessors), not from a copied list. A literal-scan test catches strings that get drawn but aren't listed. Plan 2's list is a test fixture (T3 Step 1).
- Derived numbers (`until`, `night`, `dose`) run the real engine on a reference pet. `v()` refuses rate and per-mille units, so the prose can't misread them.
- `covered-groups` lets the chapters land one at a time with the local check passing, and T15 removes it.
- `--stage` and `--pdf-only` keep each task's local check focused: chapter tasks compile the PDF from inputs already built and need no cargo. The full from-scratch build and every guard spot-check run once, in T16.
- Typst 0.15.1 is pinned in `guide/typst-version`. CI downloads the release binary, checked by sha256. Locally, the cargo fallback is used, because nix isn't installed here. The `flake.nix` entry can't be checked on this machine, and the build tells nix users when their version differs.
- The CI `guide` job builds and checks the PDF but doesn't upload it.

**Type consistency:** checked `badge_preset`, `Thresholds::FIELDS`, `Unit`, `J`, `dump`, `collect`/`GROUPS`, `menu_words`, `screen_words`, `Step`/`Shot`/`run`/`Summary`, `HERO_NAME`, `PNG_SCALE` and `set_fake_uptime_ms` across T2 to T4. On the Typst side, checked `v`, `raw-v`, `differs`, `price`, `until`, `night`, `dose`, `w`, `screen`, `sprite`, `roster`, `hero`, `pic`, `band-bar`, `need-bar`, `pixel-flag`, `diagram-box`, `tip`, `joke`, `warn`, `finish` and the labels `<gw>`, `<pic>`, `<guide-end>` and `<colophon-commit>` across T1 to T15.

**Review Focus:** items 1 to 5 are pinned in T2 and T4, T3, T4, T2, and T1, and all of them are exercised again end to end in T16 Steps 1 and 2.

**Flash:** the only firmware-side edits are host-only accessors, one `pub`, and literals moved into `const`s. The SSD1680 image should not move (±1 KiB plus the label pass's text, checked once in T16 from CI).

**Needs the owner:**
- (a) Pushing for CI makes the guide's source public on the GitHub mirror, including the US chapter, before your read. The plan doesn't upload the PDF as a CI artifact. Is the public source acceptable, or should the US chapter stay local until you've read it?
- (b) Chapters 9 to 12 are written now, in short form, describing today's badge. The index has to explain the Battle, Friends, Only pets and price words, which appear on screen today. They grow with plans 4 to 6 and 8.
