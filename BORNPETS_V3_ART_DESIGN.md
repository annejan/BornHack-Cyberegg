# BornPets v3, stage 9: new-state sprites and throwing up (design)

Plan: `BORNPETS_V3_PLAN_9_ART.md`. Roadmap row 9. Base: local `main` `5e59574`, worktree `BornHack-Cyberegg-v3art`, branch `ranzbak/bornpets-v3-art`.

This spec has three kinds of content, kept apart:
- **§1 Owner decisions**, copied verbatim. They bind.
- **§2 Findings**, measured in the tree at `5e59574`.
- **§3 Planner's calls**, each one marked *(planner's call)*. The owner may overrule any of them.

## 1. Owner decisions (2026-11-14), verbatim

### 1.1 Throw-up mechanic

**Throw-up mechanic** ("funny gross-out moment", not a harsh penalty). Sport (Exercise) while the pet is stuffed, meaning fullness above the stuffed line `needs::full_hi()` (78 % in Classic), always throws up. There is no dice.
- Fullness drops to about 45 %, so the pet is no longer stuffed.
- Weight goes down about 2 %.
- Mood takes a hit about the size of the woken-grumpy hit.
- Hydration goes down about 10 %.
- Dirt goes up: a mess, so Wash is wanted. Plan 7's floor items show it.
- The session is cut short: half the energy cost, no fitness gain, no strain or injury risk.
- Toast: "Bleurgh!". Diary cause: "threw up" / "full belly".
- Demo pets never pick Sport while stuffed.
- Battles are unchanged.
- The guide gets a line and a joke in the sport chapter.
- All numbers are tunables in thresholds, Classic and Casual, in the existing pattern.

Each "about" is a value the plan pins.

### 1.2 New art

**New art, with an OpenAI image token available.** One frame per state per pet. The states are: tipsy, wasted, hung over, sore, injured, groggy, stuffed, thirsty, wired, throwing up (10).
- The pets are Bartholomeus (prefix 00), Cat (01), Panda (05), Badger (06) and Seastar/Starfish (07).
- Owner: "The slug is hand drawn, and should not be touched, it should use the standins as today." The Slug (02) gets no new art. The Jellyfish (08) is removed by plan 7.
- Words that share a frame (controller call, owner saw it): sloppy uses the nearer of tipsy and wasted; parched uses thirsty.
- These keep today's stand-ins: wash, grubby, filthy, waterlogged, overweight, underweight, unfit, exhausted.
- Also map the art that is already on the badge but unmapped: Relaxing `0E` for Relax, and Drained `06`/`0A` for hung over and groggy. The design §8.3 table says this art exists; check whether new hung over and groggy frames replace that mapping.
- Owner: "Also update the sprite sheet definition". The `assets/bornpets-sponsors-<pet>.json5` definitions and the `assets/bornpets-sprites-<pet>.png` sheets must gain the new sequences, so that `../asset-gen` export produces the new PCX. Find which badger sheet/json5 version is live (badger, -v2 or -v3) by comparing with what is in `assets/to-badge`.
- Any pet without art for a new state, such as the Slug, shows that state's current stand-in. Today that is `to_display.rs` `display_anim`, which maps the words onto the Warning/Critical groups.

Relayed by the controller (owner, 2026-11-14): asset-gen lives at `../asset-gen` (separate git repo, HEAD `ce65374`). The sheet definition is updated through its `migrate` command: edit each pet's json5, run `asset-gen migrate` to get the re-laid-out sheet with empty slots, fill those slots with the generated art, then export. asset-gen has uncommitted owner changes. It is read-only to this stage: no edits, no commit, stash or reset. Building and running it is fine, with outputs outside its tree.

### 1.3 Space

**Space (FAT12, 2 KiB clusters, 502 total).**
- After plan 7 removes the Jellyfish there are 429 used and 73 free. Plan 7 keeps a guard of ≥16 spare, and BPETS_V3.CFG grows with plans 5 and 7 (about 7 clusters in total).
- Measured cost per pose frame is 00: 1.00, 01: 1.05, 05: 1.00, 06: 1.21 and 07: 1.37 clusters. So 50 frames is about 57 clusters, which leaves no margin.
- The plan must measure the exported PCX. If the guard would break, it simplifies the Badger and Starfish frames (less fine dither) until it fits. The guard test from plan 7 must pass.
- Also check root-directory entries (limit 511).

### 1.4 Hand-added PCX

**Hand-added PCX: never touch them.** There are 61 files: rows 0x20–0x28 of prefixes 00, 01 and 02, plus menu icons 0406xx and 0407xx. Their source configs lack those rows, so a plain asset-gen export of Bartholomeus or Cat would drop them. Copy only the new rows from a scratch export. Note also that `000000.PCX`, `010000.PCX` and `PETS.CFG` differ per badge.

### 1.5 Look and feel

**Look and feel.** Memory rule: an outline change must be uniform all round, on every frame. A partial bold outline was tried and reverted. New frames must match each pet's existing style, size, palette (black/white/red/transparent, the classification in asset-gen `template.rs::sample_frame`) and frame size. Python helpers exist in `scripts/bornpets_sheet.py`. Earlier helper scripts may be in the session scratchpad (png_to_badge_pcx.py, bold_outline.py, check_badge_pcx.py). Read them, and if the art pipeline needs them, plan to bring them into `scripts/`.

### 1.6 OpenAI key and review

**OpenAI key.** It is at the scratchpad path `openai.key`, mode 0600.
- Plan generation scripts that read it from a path given on the command line or in an env var.
- Never print it, log it, commit it, or put it in the repo.
- Use the current OpenAI image model and API, and look up current docs if needed. Include an owner-visible review step: a contact sheet PNG of all new frames per pet, so the owner can look at them before export.

Owner, later the same day (supersedes the contact-sheet form of the review step): "When you have generated the sprite sheets, show them to me in a gallery so I can review them and give feedback." Relayed by the controller:
- Per pet, the generation task writes the migrated sheet PNG with the new frames filled in.
- Per new frame it writes a PNG crop, upscaled about 4x with nearest-neighbour, with the same pet's existing idle frame beside it for comparison.
- A `manifest.json` lists pet, state, anim id, file path and the cluster cost after PCX export.
- Everything goes under one directory. The controller builds the gallery (a private claude.ai Artifact with a per-frame feedback field). The plan builds no web page.
- Gate: no PCX goes into `assets/to-badge` and no sheet is committed as final until the owner has reviewed the gallery. The loop is: owner feedback per frame, regenerate only the flagged frames, new gallery round. At most 3 rounds, then the controller asks the owner.

Owner, later the same day: "use other frames from the sprite sheet to get good consistency." Relayed by the controller:
- Every new frame is made from the pet's own existing frames as reference images, not from a text prompt alone, through the current model's image-edit reference input.
- The reference set per pet is the idle frame plus the 2–4 existing frames closest to the new state. Examples: tipsy/wasted use the drinking and miserable frames, injured/sore use critical sick, stuffed uses feeding, groggy and hung over use the drained/tired frames, and throw-up uses critical sick and feeding. The plan names the set per state in a table.
- Post-processing forces each output onto the exact frame size and the black/white/red/transparent palette.
- The outline must be uniform all round on every frame. A check step compares the outline width and the pet's bounding box against the idle frame, and flags frames that drift so the gallery can mark them.

### 1.7 Anim ids and RAM

**Anim ids and RAM.** `src/game/sprite_loader.rs` has `AA_MAX = 41` (ids 0x00..=0x28) and a RAM-tight debug build. MANIFEST.TXT reserves 0x15–0x1F for env/icon features; Bartholomeus and Slug ship files 0015xx–001Exx (`ENV_*`/`ICON_*`, `PLACEHOLDER`).
- Find out whether the firmware uses those, and choose ids for the 10 new states.
- Cost the RAM of growing `AA_MAX` against reusing reserved ids.
- If reusing changes or deletes shipped files, say so and keep it inside plan 7's `copy_assets.py --update` replace/remove mechanism.

### 1.8 Rules for every task

Delta engine, no polling. A throw-up is one event inside the Exercise dose. Focused tests per task, then a CI regression on the GitHub mirror and a bug-fix round. Cargo uses 2 cores. Integer math, no alloc. Never write kv `state`, `game/name` or `BORNPETS.CFG`, and the save is `state_v3`. Toast ≤ 15 columns, cause what/why ≤ 12/≤ 15. Every new on-screen word goes in a `words()` list, and `make guide` must pass. Never touch GAME.md. R15: code made dead is removed in the same commit.

### 1.9 Floor litter (owner, 2026-11-14, relayed by the controller)

The quoted phrase is verbatim; the rest is the controller's relay, kept close to its words.

"dirt items indeed are the floor litter items": the floor litter, meaning plan 7's crumbs, peel, can, sock and pizza box, must also be made in the pets' sprite style, using existing sprite-sheet frames as references so the style stays consistent.
- Part A generates the 5 litter items with the same reference-frame method, palette snap (black/white/red; transparent where the item doesn't cover) and outline rule. They go into the same owner gallery rounds (manifest `pet = "floor"`).
- The item size is the planner's to propose: the current slots are tiny. The proposal stays within the floor strip and clear of the UI (plan 7's layout rules), and the gallery shows it as the planner's call.
- Storage: the controller recommends baking the litter into firmware flash as const bitmaps (two 1-bit planes, about ≤ 100 B each), not as PCX on FAT12.
- Part B replaces plan 7's hand bitmaps with the baked art. It keeps `scene.rs` and the dirt thresholds unchanged, and updates plan 7's pixel tests and the guide shots/text as needed.

### 1.10 Amendment 1 (owner, 2026-11-15), verbatim

A4 stopped on space: 50 frames cost 72 clusters, the limit was 55 and 22 frames were over 2048 B. The owner decided the following (quotes are verbatim).

1. **Space: "Free + simplify".**
   - Remove the 18 Drained files (anim rows `06` and `0A` of prefixes 00, 01, 05, 06, 07) from the badge volume. The Slug's `020600` / `020A00` are not touched.
   - Redraw frames over 2048 B with flatter fills until the art fits. Keep every state.
   - The Drained cells stay in the sheets and remain usable as references.
2. **Relax frame 3.** "Relax has 3 frame animations now, that can be 2." Remove `050E02`, `060E02`, `070E02` the same way. No Slug file is touched.
3. **The Slug is in scope.** "The artist gave permission to create the extra art for the slug as well." Its existing files and sheet rows stay byte-identical.
4. **Full set at once.** "Starving" and "needs rehab" are new states for all six pets. The target is 6 x 12 = 72 frames, budgeted once. The flat-fill style is decided up front and applied to every generation.
5. **Bartholomeus notes** (owner):
   - hung over: "Make it tilt it's head forwards because it has a headache."
   - injured: "No cast or band aid?"
   - needs rehab: "More dramatic, holding on to something to keep itself standing, not just the same image nothing changed."
   - starving: "Can you make it look starved? it's a bit like it's always."
6. **Keep the A4 work.** The 50 A4 frames are kept. Only frames over 2048 B are regenerated, plus the two noted Bartholomeus redraws.
7. **Merge first.** Merge local `main` `a718cc0` (plan 7 merged, Jellyfish gone) into the branch before anything else.

These supersede §1.2 "The Slug (02) gets no new art", §3.2 "Drained `06`/`0A` stay ... their files stay on the badge", the plan's "Needs the owner" question (answered: yes) and the Global Constraints "Space: ... at most 55 clusters".

## 2. Findings at `5e59574`

1. **Live badger sources.** `bornpets-sponsors-badger.json5` with `bornpets-sprites-badger.png` (no suffix). Decoded pixel by pixel, all 51 shipped `06*.PCX` match its sheet. The `-v2` and `-v3` sheets match 0 of 51; they are stale.
2. **The other sheets reproduce what ships.** Panda 48/48, Seastar 50/50. Bartholomeus 36/38 and Cat 41/43: the four misses are the per-badge start screens `000000`/`010000` and the placeholders `001E00`/`011F00`, which differ from their sheet rows. So even a full export of these two pets would overwrite four files besides dropping the 61 hand-added ones. Only the new files are copied (§1.4).
3. **Row layout.** asset-gen numbers rows by their position in the json5. Panda, Badger and Seastar end at row `0x28`. Bartholomeus ends at `0x1E` and Cat at `0x1F`; rows `0x20`–`0x28` of those two exist only as hand-added PCX.
4. **Reserved ids 0x15–0x1F are unused.** No firmware code reads them. They are shipped as files: `001500`–`001E00` (10 files), `011F00` (1 file) and `021500`–`021C00` (8 Slug files).
5. **RAM.** The presence table `ANIM_PRESENCE` is `PP_MAX 9 × AA_MAX 41 × 2 B = 738 B` of static RAM.
6. **Space.** Shipped PCX cost per prefix (clusters per file): 00 1.02, 01 1.05, 05 1.02, 06 1.20 (10 of 51 files over 2048 B), 07 1.36 (18 of 50 over). A PCX of at most 2048 bytes is exactly one cluster. `MANIFEST.TXT` averages 31 B a line. `BPETS_V3.CFG` averages 38 B a key row (282 keys, 11 208 B at `5e59574`).
7. **Existing pipeline.** `scripts/bornpets_ai_frames.py` already does anchor → `/v1/images/edits` → quantize (the `sample_frame` classification) → assemble, with `curl -K -` so the key never reaches argv. It has profiles for badger, seastar and jellyfish only. It defaults to `gpt-image-2` and asserts that each pet's rows match the Panda's row layout. `png_to_badge_pcx.py` and `check_badge_pcx.py` are already in `scripts/`. The scratchpad copy of `bold_outline.py` is a Seastar-only partial bold outline, the reverted approach.
8. **Current OpenAI image models** (developers.openai.com, read 2026-11-14): `gpt-image-2` is marked deprecated. The docs say "For new integrations, use GPT-Image-2.6 Sunburst or Flare": `gpt-image-2.6-sunburst` (detailed work) and `gpt-image-2.6-flare` (fast). Quality `low|medium|high|xhigh|max|auto`; recommended sizes `1024x1024`, `1536x1024`, `1024x1536`; `input_fidelity` is not sent.
9. **The dose lands at the action's end** (`finish_action` → `doses::apply`), and needs do not age during an action (R23). So "stuffed when Sport starts" and "stuffed when the dose lands" are the same test.
10. **Plain cause codes are full.** Codes 0 to 15 are used and `TORN_BASE` is 16. Cause codes never reach flash: the diary is RAM only.

## 3. Planner's calls

### 3.1 Anim ids and the presence table *(planner's call)*

| id | State | `DisplayAnim` | id | State | `DisplayAnim` |
|---|---|---|---|---|---|
| `0x29` | tipsy | `Tipsy` | `0x2E` | groggy | `Groggy` |
| `0x2A` | wasted | `Wasted` | `0x2F` | stuffed | `Stuffed` |
| `0x2B` | hung over | `HungOver` | `0x30` | thirsty | `Thirsty` |
| `0x2C` | sore | `Sore` | `0x31` | wired | `Wired` |
| `0x2D` | injured | `Injured` | `0x32` | throwing up | `ThrowingUp` |

Relax gets `DisplayAnim::Relaxing` on the existing id `0x0E`.

The RAM options were costed against each other:

| Option | Static RAM | Shipped files changed |
|---|---|---|
| Grow `AA_MAX` to 51 | +180 B (918 B) | none |
| Reuse `0x15`–`0x1E` | 0 B | 18 to 19. The Slug's 8 files would show as its "tipsy" etc. and must be deleted, which touches the Slug. Bartholomeus' 10 would be replaced. A badge whose copies do not hash-match keeps them and shows a 20 x 20 food icon as a pose. |
| **Chosen:** ids `0x29`–`0x32`, presence table indexed through a slot map that skips the unused `0x15`–`0x1F` | **−18 B (720 B, 40 slots)** | none |

The slot map is `0x00..=0x14 → 0..=20` and `0x20..=0x32 → 21..=39`. Ids `0x15`–`0x1F` are not catalogued; no code reads them (§2.4). Their files stay on the badge untouched.

### 3.2 What the pet shows *(planner's call)*

- **Source of the picture.** The art follows the first word of the status line (`status_line(s).first`), so the picture matches the word under it. The stand-in stays exactly today's `display_anim()` (from `status()`), so the Slug and any pet without a file look as they do today.
- **Word to frame:**
  - tipsy → Tipsy;
  - sloppy and wasted → Wasted (the Classic sloppy band 38–70 has its middle at 54, which is 16 from the wasted line and 44 from the tipsy line; a fixed mapping, so no new solver line);
  - hung over, sore, injured, groggy, stuffed and wired → their own frame;
  - thirsty and parched → Thirsty.
  - Every other word keeps its stand-in.
- **Priority.** Only the content and status groups (today's groups 4 to 6) are replaced. An action, sleep, leaving, hatching, gone and hibernating always win, and the sleep display floor still applies.
- **Throwing up.** It shows for the whole Exercise action when the pet was stuffed. It needs no new state (§2.9). Without the file, the stand-in is today's Exercising.
- **Relax** shows Relaxing `0E` for every pet that has it, including the Slug's own hand-drawn `020E` frames, which are not touched. Without it, the stand-in is today's Playing. Wash keeps Playing (owner, §1.2).
- **Drained `06`/`0A`.** The new hung over and groggy frames replace that mapping. The Drained art shows an empty battery, which reads as "tired", not as a hangover. Only the Slug would ever use it, and the Slug keeps today's stand-ins. So `06`/`0A` stay unmapped and their files stay on the badge.
- **Exactness.** The shown animation is a function of the status line, the action and the fullness of a running Exercise. All of these are already in the engine's `Visible` view (fullness does not change during an action). The wake stays exact with no new line or timer.

### 3.3 Throw-up numbers *(planner's call; owner's "about" values pinned)*

| Key | Classic | Casual | Meaning |
|---|---|---|---|
| `THROWUP_FULL_PCT` | 45 | 45 | fullness is set to this; `validate`: `FULL_LO_PCT < v < FULL_HI_PCT`, else preset |
| `THROWUP_WEIGHT_PCT` | 2 | 2 | weight drop |
| `THROWUP_MOOD_PCT` | 10 | 10 | mood drop (= `GRUMPY_PCT`, the grumpy-wake hit) |
| `THROWUP_WATER_PCT` | 10 | 10 | hydration drop |
| `THROWUP_DIRT_PCT` | 30 | 30 | dirt rise; dirt also ends at least on the first floor-item line (plan 7 `FLOOR_CRUMBS_DIRT_PCT`, 40), so the mess shows and Wash is open |
| `THROWUP_ENERGY_SHARE_PCT` | 50 | 50 | share of `DOSE_EXERCISE_ENERGY_PCT` the cut-short session costs |

Casual equals Classic. Its later stuffed line (83) already makes it gentler, as with `GRUMPY_PCT`.

- **Trigger.** Strictly above the line: `fullness > needs::full_hi()`.
- **What a throw-up does not change.** Fun, booze, hangover, groggy, fullness loss from sport, fitness and strain all stay as they are. Leaving booze alone keeps throwing up from becoming a way to sober up.
- **Session length.** The action keeps its `EXERCISE_DURATION`. "Cut short" is in the effects.
- **Texts.**
  - New `Cause::ThrewUp`: `what` "threw up", `why` "full belly", toast headline "Bleurgh!" through a new `Cause::toast_what`.
  - The toast reads "Bleurgh!" over "full belly", and the diary "threw up" / "full belly". The menu's risk title reads "risk: threw up" (the one `what` text, like "risk: torn muscle").
  - Pack code 64, a new range after `BAD_NIGHT_BASE` (48 + 8).
- **Menu and hint.** The menu and hint warn for free. `preview` runs the real dose on a clone, so Sport on a stuffed pet is marked risky, and the hint prefers a clean remedy when one exists. This is existing behaviour: nothing is added.
- **Demo mode.** The demo pet never exercises (plan 3), which covers the owner's rule. A test pins it so a later policy change cannot break it.
- **Save.** No save byte. The throw-up is decided from saved fields, and the cause queue is RAM only.

### 3.4 Art pipeline *(planner's call)*

- **Generator.** Extend the existing `scripts/bornpets_ai_frames.py`; no second generator.
  - Profiles are added for bartholomeus, cat and panda.
  - The 10 state briefs are shared, with pet-specific props from each profile's freedom text.
  - The default model becomes `gpt-image-2.6-sunburst`.
  - The key is read from `--key-file PATH` or `OPENAI_API_KEY_FILE`. The script refuses a key file inside the repo or one readable by group or others.
- **Reference images (owner, §1.6).**
  - Every state frame is one `/v1/images/edits` call with the pet's idle frame as `image[]` 1 and the frames of the table below as further `image[]` entries (at most 5 in all; the docs take several, and the first is the primary).
  - The references are decoded from the shipped `assets/to-badge` PCX, which is the badge truth and also covers Bartholomeus' and Cat's hand-added rows `0x23`/`0x25`.
  - Each reference is pasted at one fixed integer scale and position on the request canvas. The output is cut back out of that same rectangle and scaled down by the same factor, so the new frame keeps the idle frame's size and place exactly.

| State | References after idle (frame 0 of each row) |
|---|---|
| tipsy | drinking `25`, happy `02` |
| wasted | drinking `25`, warning miserable `0B`, critical tired `04` |
| hung over | critical drained `06`, warning sick `07`, drinking `25` |
| sore | critical sick `03`, exercising `23`, warning sick `07` |
| injured | critical sick `03`, healing `0D`, exercising `23` |
| groggy | warning drained `0A`, warning tired `08`, sleeping `10` |
| stuffed | feeding `0C`, warning miserable `0B` |
| thirsty | warning hungry `09`, drinking `25` |
| wired | playing `0F`, happy `02`, exercising `23` |
| throwing up | critical sick `03`, feeding `0C`, exercising `23` |

- **Post-processing.** The palette goes through the existing `classify` (the `sample_frame` rules) and `despeckle`. The size is exact by the cut-back (above).
- **Drift check.** It compares each new frame with the pet's idle frame. Each check flags the frame for the gallery:
  - median outline width, from the silhouette inward: drift of 1 px or more;
  - outline coverage (share of silhouette edges that start black): more than 10 points below idle;
  - the pet's bounding-box height (its largest opaque 8-connected part): more than 15 % off;
  - the bounding-box centre: more than 8 px off;
  - any art cut off at the rectangle edge.

  A flagged frame is regenerated, never patched: no automatic outline edit, because a partial outline change was reverted (§1.5).
- **Not brought into `scripts/`.** `bold_outline.py` (the reverted outline approach) and the ad-hoc scratchpad viewers stay out. A new `scripts/bornpets_art_check.py` does the export diff, the space budget and the review files.
- **asset-gen.** Use the existing `../asset-gen/target/release/asset-gen`, which is newer than its sources. Build it once, out of tree, only if that binary lacks `migrate`. A round-trip export check proves that `migrate` changed no existing frame.
- **Space rule.**
  - Budget for the 50 new PCX: 73 free after plan 7, minus 1 for `BPETS_V3.CFG` (7 clusters with plan 5's 48 keys and these 6), minus 1 for `MANIFEST.TXT` (+50 lines, 5 to 6 clusters), minus the 16 guard = **55 clusters**.
  - Every frame over 2048 B is simplified, Badger and Starfish first. The simplifier only merges small enclosed islands of one colour into the colour around them. It never touches transparency or anything next to it, so the silhouette and outline stay as they are (§1.5).
  - Root entries: 372 + 50 = 422 of 511.
- **Shipping.** Only the 50 files `PP29xx`–`PP32xx` for PP 00, 01, 05, 06, 07 are copied. `MANIFEST.TXT` is hand-edited and its new version listed in plan 7's `asset-changes.txt` `replace` line. New files need no change line, because `--update` adds missing files.
- **Review directory.** `target/v3art/review/` in this worktree (gitignored).

### 3.5 Floor litter *(planner's call)*

**Plan 7 at `7fdea40`** (`../BornHack-Cyberegg-v3bg`, its Task 3 landed):
- `src/game/background.rs` draws five black-only `u16` bitmaps (`CRUMBS`, `PEEL`, `CAN`, `SOCK`, `PIZZA_BOX`, in `FLOOR_BITMAPS`) into `FLOOR_SLOTS`: 7 x 3, 11 x 6, 10 x 6, 9 x 8 and 16 x 5.
- The floor strip is y 100 to 109 (10 rows). It is white day and night: the night dither ends at y 99 (`SKY_BOTTOM`), and the tufts are skipped wherever a shown item meets them. So under a litter item there is only white floor.
- Its rules: every slot inside the strip and x 0 to 151, no two slots overlapping, items cumulative and never moving, grass tufts skipped where an item is shown, and no red outside the poorly cross.
- The status line, with its backing, is drawn over the floor and wins. The toast, HAX, battery, demo mark, sun and cross all sit above the strip.

**Slots** (bottom row y 109; left and right groups, as plan 7 has them):

| Item | Plan 7 slot | Proposed slot (x, y, w x h) |
|---|---|---|
| crumbs | (4, 107) 7 x 3 | (2, 105) 12 x 5 |
| banana peel | (120, 104) 11 x 6 | (116, 101) 16 x 9 |
| can | (16, 104) 10 x 6 | (16, 100) 12 x 10 |
| sock | (136, 102) 9 x 8 | (134, 100) 14 x 10 |
| pizza box | (29, 105) 16 x 5 | (30, 102) 24 x 8 |

- The left group spans x 2 to 53 and the right group x 116 to 147; no two slots touch.
- Ten rows is the strip's full height. A taller item would reach into the sky, which is dithered at night.
- The widths leave room for a 1 px outline around a readable shape at the pets' scale (the 20 x 20 `ENV_*` icons are the nearest shipped examples).
- The gallery shows the strip with all five items, the plan-7 slots beside them, and a pet standing over them.

**Palette: black, white and transparent; no red.** This narrows the owner's "black/white/red":
- Plan 7's battery rule says red appears only while the red pet line already asks for a full refresh, and its test `only_the_cross_is_red` pins that.
- Red litter would put red on the screen without the red line, and on the SSD1675 red needs a full refresh to seat.
- Any red the model draws is snapped to black, and the gallery flags it.

**Method** (as for the pets, §3.4: a fixed rectangle, reference frames, a cut-back by an exact factor):
- One `/v1/images/edits` call per item, on the same 1536 x 1024 magenta canvas.
- **Image 1 (primary)** is plan 7's hand bitmap for that item, stretched nearest-neighbour onto the new slot and drawn at the item scale `S = min(1400 // w, 900 // h)` (crumbs 116, peel 87, can 90, sock 90, pizza box 58) in a rectangle centred on the canvas. It plays the idle frame's role: it fixes what the item is, its size and its place.
- **Images 2 to 4** are shipped frames decoded from `assets/to-badge`, each at the largest integer scale that fits the canvas, centred, transparent as magenta. The Slug's `021500`–`021700` are only read, as references; they are never modified (§1.2):

| Item | References after the plan-7 sketch |
|---|---|
| crumbs | `001500` (Bartholomeus food item), `050C00` (Panda feeding), `021500` (Slug food item) |
| peel | `001500`, `010C00` (Cat feeding), `021500` |
| can | `052500` (Panda drinking), `062500` (Badger drinking), `001600` (medicine) |
| sock | `001700` (toy), `021700` (Slug toy), `051000` (Panda sleeping) |
| pizza box | `001500`, `060C00` (Badger feeding), `021500` |

- The prompt asks for chunky pixel art on a w x h grid with a 1 px black outline, matching the references' line weight, and no red.
- **Cut-back.** The output's rectangle is cut out and reduced by exactly `S` (`Image.BOX`). Then a cell is black when at least 25 % of it is dark (so a thin drawn outline survives the reduction), else white when at least half of it is opaque, else transparent. Red is snapped to black and flagged. Then `despeckle`.
- **Drift check** (flags for the gallery, never an automatic patch). Measured on the shipped references: the 20 x 20 icons `001500`/`001600` have a 1 px outline with 100 % black edges.
  - the outline median is not 1 px;
  - black edges under 90 %;
  - no ink on the slot's bottom row (the item must rest on the floor);
  - ink under 15 % of the slot (the item would read as a speck);
  - any red snapped.

**Storage: baked into flash, as the controller recommended, but as one plane, not two.**
- **Why baked.** FAT12 has no margin. The 50 state frames already need all 55 spare clusters, and five more PCX would cost 5 clusters plus a loader.
- **Why one plane.** Under an item there is only white floor (above), so a white plane would draw white on white: 92 B of flash that changes no pixel. Transparent and white inside the outline look the same on the badge.
  - The source PNG keeps white and transparent apart. If a later plan paints the floor, a `paint` plane is 92 B more and the generator already has the data.
  - The gallery shows each item on the white floor, which is what the badge shows.
- **Flash cost:**
  - One 1-bit `ink` plane per item, rows padded to whole bytes, MSB first: crumbs 10 B, peel 18 B, can 20 B, sock 20 B, pizza box 24 B, so **92 B**.
  - Plan 7's five `u16` bitmaps are removed (56 B). `bitmap()` keeps its size: it reads byte rows with a stride instead of `u16` rows, and the tuft becomes byte rows too (8 B → 4 B).
  - Net about **+32 B** (under 0.1 KiB). Plan 9 then totals about +1.6 KiB, still about 878 KiB of 960 after plans 5 and 7.
- **Generated code.** `scripts/bornpets_litter.py` generates `src/game/litter_art.rs` (the five slots and the five ink planes) from one source image, `assets/bornpets-litter.png`. That image is the 152 x 10 floor strip with each item in its slot, in the four badge colours. It is not an asset-gen sheet and is never exported to `assets/to-badge`. A test fails when the generated file is stale.

**Part B** replaces `CRUMBS` … `PIZZA_BOX`, `FLOOR_BITMAPS` and the slot values:
- `scene.rs`, `FLOOR_ITEMS`, the tufts, the cross and the `FLOOR_*_DIRT_PCT` keys stay as they are.
- `FLOOR_SLOTS` keeps its name and type; it moves into the generated file and `background.rs` re-exports it.
- No plan-7 test changes its code. Every one is written over `FLOOR_SLOTS` and its bounds, so they are re-run on the new data. The list is in the plan, Task B4b.
- The guide keeps plan 7's "floor" shot and ch3 table. The shot is redrawn and looked at; the text changes only if it describes how an item looks.

### 3.6 Amendment 1: tooling for six pets, twelve states and the computed limit *(planner's call)*

- **Ids.** `STATES` gains `needs_rehab` (`0x33`) and `starving` (`0x34`), appended, so `STATES[i]` still has id `0x29 + i`. That is 12 states, ids `0x29`-`0x34`, and 72 new files (`new_files()`, six pets). Part B maps them for every pet with the files; a pet without a file falls back to today's stand-in through `shown_anim`.
- **The Slug** joins `ART_PETS` (prefix 02, `bornpets-sponsors-slug.json5`, `bornpets-sprites-slug.png`, sequence `slug`). Its sheet gains rows `0x1F` (reserved), `0x20`-`0x28` (blank hand-added boxes, as for Bartholomeus and Cat) and `0x29`-`0x34`, via `asset-gen migrate`. Every existing `02xxxx` file below `0x29` is protected: `diff` fails (`protected_changed`) when one changes or vanishes. `021500`-`021700` stay litter references only.
- **Briefs and references.**
  - needs rehab (refs `0x27`, `0x04`, `0x0B`): the pet can barely stay upright, knees buckling, shaking, holding on with both arms to something solid (a post, a railing or a walking frame), heavy eyes, a few wobble lines.
  - starving (refs `0x05`, `0x09`, `0x0C`): visibly thinner and droopier than idle, sunken eyes and cheeks, holding out an empty bowl, a small rumble mark by its belly.
- **`FLAT_FILL`.** Every state prompt carries, after the style text and before any owner note: "FLAT FILLS, SMALL FILE: large areas of one flat colour; no dithering, no hatching, no texture, no stippling, no speckles or scattered dots; few small details; every outline a continuous unbroken line. Solid white stays solid white." Frames that already fit are not regenerated.
- **Retirement.** `RETIRED` lists 21 names: the 18 Drained files and `050E02`, `060E02`, `070E02`. They leave `assets/to-badge` through `remove` lines in `asset-changes.txt` (every known version hash), and their `MANIFEST.TXT` lines are deleted by hand. The sheets keep the cells.
- **The limit is computed, not assumed.** `volume_limit() = 502 - 16 - (used(to-badge) - clusters(RETIRED still present)) - cfg_growth - manifest_growth`, with `cfg_growth` the clusters `BPETS_V3.CFG` gains up to 13 404 B (plan 5's 13 176 B plus six `THROWUP_*` rows) and `manifest_growth` the clusters `MANIFEST.TXT` gains when the retired lines go and the 72 new lines (2 040 B) come. The 16-cluster guard is unchanged. `NEW_PCX_BUDGET` and `ASSUMED_USED` are gone.
- **Sheet references.** `ref_rows(prefix, aa)` decodes the shipped PCX when it exists; otherwise it crops frame 0 of row `aa` from the pet's sheet and maps it through `classify` to indices 0-3, so retired Drained rows still work as references (hung over uses `0x06`/`0x07`, groggy `0x0A`).
- **Gallery.** The A4 files were never shown to the owner, so the amended gallery (A5) is round 1 and the 3-round cap counts from there.
