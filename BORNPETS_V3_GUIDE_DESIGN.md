# BornPets v3 player's guide: design

Status: draft for owner approval (2026-11-13). The roadmap gets a new stage for it, "Player's guide", that runs alongside the owner's hardware testing.

## Intent

The v3 rules are much richer than v1. There are nine needs with comfort bands, a sleep with no timer, booze and hangovers, dependence and rehab, diabetes, injury, mood and health ceilings, and refusals with reasons. A player who doesn't know the rules can't read the pet, and a pet that can't be read feels random.

The owner wants a proper player's guide, "like there used to be with games you bought", in the style of the **Amiga-era game booklets**: a short story up front, then loading and controls, then the world explained page by page, with pixel art in the margins and a sense of humour. Lemmings and Populous are the reference.

Success: someone who just got their badge at the gate can hatch and keep a pet alive from the first pages. Someone keeping a pet alive all week finds the details (bands, conditions, what each action really does) in the back.

## Owner decisions

- **Format:** a PDF with graphics.
- **Style:** Amiga-era game booklet.
- **Readers:** both the first-timer at the gate and the week-long carer.
- **Hard mode (US):** it gets its own chapter, with **US flags and US stereotypes**.
- **Timing:** the guide is written **while the owner has the new firmware on the badge and is testing**, so it describes the game exactly as it behaves on the badge. Later plans extend or correct their chapters when they land.
- **Labels:** "If the labels in the game need to be changed to make the game flow more clear to the player, do so." A label that needs a paragraph of explanation in the guide gets renamed in the firmware instead. The guide and the firmware change together.
- **Alarms:** the diabetes and dependence alarms fire again on every boot, unless the pet is hibernating. The guide says so.

## Goals

1. The first pages get a new player from "badge in hand" to "pet hatched, fed and asleep", with no other reading needed.
2. Every on-screen word gets explained somewhere: status words, banners, menu rows, toasts, refusal reasons and the Engine info line. Every one appears in an index at the back, with its page number.
3. Every number the guide states matches the firmware the owner is testing. Numbers come from the code and the presets, never from memory.
4. Pixel art is everywhere: the real pet sprites, real screens from the simulator, and booklet-style drawings of the comfort bands.
5. The tone is light and joking, but never at the expense of accuracy.

## Non-goals

- No firmware feature that isn't on the badge. Chapters for plans not yet built (stations, battles v3, demo mode, Hard mode v3 bills) describe what the badge does today. They get rewritten when their plan lands.
- No balance tuning. The guide reports the rules and does not change them. Label renames are the one exception, and they don't change any rule.
- Not a replacement for the technical docs (README, QUICKSTART, USER_GAMES). The guide points to them for flashing and the config file.

## Anti-goals (a failure even if it builds)

- A rule the badge doesn't follow, or a number that's wrong. A player who trusts the guide and loses a pet to it is the worst outcome.
- A wall of text. If a page has no picture, the page is wrong.
- Jokes that punch at people. The US satire aims at the system: insurance paperwork, surprise bills, the copay, eviction, plus harmless clichés (giant portions, bald eagles, flags). *(builder's call: owner may sharpen)*

## Contents (chapter outline)

1. **The legend.** A short story: why there are pets on badges at BornHack, and what the Unicorn Realm is. One page.
2. **Getting started.** Turning the badge on, the buttons, choosing a pet kind and mode (with money, without money, Hard (US)), hatching, naming.
3. **Reading your pet.** The screen layout, the status word, `!` (danger), the banners, the buzz. When the screen wakes and why it sleeps.
4. **The nine needs.** One spread per need, or grouped: fullness, hydration, energy, fun, dirt, booze, weight, fitness and strain. For each one: its comfort band drawn as a bar, what pushes it up and down, and what happens past the edges.
5. **Care actions.** Feed, Water, Sleep and Wake up, Relax, Wash, Play and the mini-games, Heal, Ozempic, Medicate and Rehab. What each one really does, from the dose table, and every reason it can be refused.
6. **Mood and health.** The mood target, the health ceiling, how damage and recovery work, collapse, leaving, and Gone.
7. **Conditions.** Hangover, groggy, sleep-deprived, injury, dependence and rehab, diabetes and its reversal. How you get each one and how you get rid of it. The boot alarms.
8. **Off-time and hibernation.** What happens while the badge is off (catch-up up to 12 h once the clock is known), and what hibernation does.
9. **Money and HAX.** Prices, work, and the mini-game rewards.
10. **Hard mode (US).** Flags, stereotypes and satire, covering what the badge does today. It gets extended with plan 6 (deductible, copay, surprise bills, the authorization popup, eviction).
11. **Friends, battles and stations.** What's on the badge today: friend boosts, battles, the NFC "magic places" and their phrases. Rewritten with plans 4 and 5.
12. **The Unicorn Realm.** Past pets and generations.
13. **Secrets.** The Konami code and the debug menu, as a hidden page.
14. **For hackers (appendix).** `BPETS_V3.CFG`, Classic and Casual, how to tune, and a pointer to USER_GAMES.md. How to go back to the old firmware (it keeps `BORNPETS.CFG`, the `state` save and the name).
15. **Index of on-screen words.**

## Constraints

- **Source of truth:** the firmware on branch `ranzbak/bornpets-v2` as tested by the owner, then `BORNPETS_V3_DESIGN.md`. Where they differ, the firmware wins and the difference gets reported to the owner.
- **Art:** only the repo's own assets: the pet sprite sheets in `assets/`, the menu icons, and screens rendered from the simulator. Sponsor art only as the sponsor files allow. No AI-generated art without the owner's yes.
- **Language:** English, short sentences, plain words.
- **Must not touch:** the firmware rules, saves, the old firmware's keys and files, and the hand-added PCX files on badges.
- **Build machine:** at most 2 cores for local builds, and one cargo command at a time.
- **Licensing:** the guide's source lives in the repo, and the PDF is built from it reproducibly with one command.

## How it gets made (builder's calls, stated)

- **Layout tool (owner chose, 2026-11-13):** **Typst**, installed with `cargo install typst-cli --locked` and built on 2 cores (the usual `CARGO_BUILD_JOBS=2 taskset -c 0,1 ... -j 2`). typst also goes into `flake.nix` for other contributors.
- **Page format:** A5 portrait. The page count is padded to a multiple of 4, so the same PDF can be printed as a folded, stapled booklet if the owner ever wants copies to hand out. Download-only works too.
- **Look:** cream page and dark ink, a pixel display font for headings, a readable body font, chunky borders, sprites at whole-number scale (no smoothing), and sidebar boxes for tips and jokes.
- **Screens:** a small simulator option that saves the current screen as a PNG, plus a script that drives the simulator to each state. The debug-menu pokes reach every status word in seconds. Screens are rebuilt whenever the firmware changes.
- **Numbers:** a script reads the band edges, doses and timers from the code or the template `BPETS_V3.CFG`, and writes a data file the layout includes. Only the prose is hand-written.
- **Where:** a `guide/` directory in the repo, with the source, scripts and fonts (with their licences). The built PDF goes to `guide/out/`, and the published copy is an Artifact or release asset when the owner says so.
- **Label pass:** while writing each chapter, any on-screen word that needs explaining gets considered for a rename. Renames ship as separate small firmware commits, with tests updated, on the same branch. Each one gets listed for the owner.

## Process

1. The owner flashes plan 1 and starts testing.
2. The first pass covers chapters 1–8, 13, 14 and 15, which are on the badge now, plus the label renames.
3. The owner reads the PDF next to the badge and corrects it ("it doesn't say that, it says…").
4. Chapters 9–12 get their full version as plans 2–6 land. The roadmap row for each plan adds "update the guide chapter".

## Settled after the builder check (controller's calls, marked)

- **Which firmware *(my call)*.** The guide tracks the head of `ranzbak/bornpets-v2`. One command rebuilds the data file, the screens and the PDF, and the guide is rebuilt after every firmware change. The colophon prints the firmware commit hash the PDF was built from, so the owner knows which build a copy describes. A difference between the code and `BORNPETS_V3_DESIGN.md` goes in the builder's report; the guide follows the code.
- **Label renames *(owner authorized; timing is my call)*.**
  - Renames touch on-screen text only: menu rows, toasts, status words, reasons and banners. They never touch rules, save bytes, kv keys or config keys.
  - They are batched once per chapter pass, applied, and listed for the owner (old → new, and why). They land *before* the owner's read of that pass, so the PDF and the badge match when compared.
  - A rename after the owner has started comparing a chapter goes into the next pass.
- **Screens *(my call)*.**
  - The desktop simulator (`src/bin/simulator.rs`, embedded-graphics-simulator) already draws the same frame as the badge, in the same black-and-white e-paper mode and at the same resolution.
  - Add a simulator-only screen-dump option (`--features simulator`; it never builds into the firmware) and a script that drives the simulator through the v3 debug menu (Task 16 pokes, skips and Engine info), which exists on the branch.
  - This is simulator code, not a firmware rule change, so it is allowed.
  - Photos of a real badge are an optional extra for the cover and the getting-started chapter.
- **Numbers *(my call)*.**
  - A small host-only dump, an ignored test or a `--features simulator` bin, prints `Thresholds::CLASSIC` and `Thresholds::CASUAL`, the dose values and the timer constants as JSON. The layout reads that file.
  - Classic is the default the badge ships with, so it is "the" number in the prose. Casual gets a column in the appendix and a sidebar wherever it differs.
  - Prose refers to numbers by key, never as typed literals. A key missing from the data file fails the build.
- **Unbuilt features *(my call)*.** Leave them out. No "coming soon" pages: what the badge doesn't do isn't in the guide. Chapters 9–12 describe today's behaviour and grow with their plans.
- **Pets and art *(my call)*.**
  - A "Meet the pets" spread shows every kind in `PetKind::roster()`, plus one hero pet who walks through the examples.
  - Sponsor art stays out, because its licence and the sponsors' consent to print are unknown. The owner can add it later.
- **Review and acceptance *(my call)*.**
  - The owner's read against the badge is the acceptance gate. It covers the US chapter text too: nothing gets handed out without that read.
  - A first-timer check, where someone who never saw the game hatches a pet using only the first chapters, is recommended but optional.
  - The builder adds the "Player's guide" stage row to `BORNPETS_V3_ROADMAP.md`, and an "update the guide chapter" line to plans 2–6.

## Left to the builder

The exact story text, the jokes, the fonts (free licences only), the colours, the page-by-page layout, and which screens to show.
