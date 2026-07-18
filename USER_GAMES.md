# BornPets & Mini-Games — User Guide

The **Game** screen runs BornPets — a virtual pet inspired by Tamagotchi — plus five mini-games you can launch from the pet's **Play** menu.

## BornPets

Hatch a pet, then keep it fed, healthy, rested and entertained.

### Hatching

The very first time you open Game you see the hatchery. Push **EXE / Fire** to start. Pick your pet — **Bartholomeus** (a snail), **Cat**, **Slug**, or **Panda** — then wait about a minute for the egg to hatch. After hatching you name your pet — up to 12 characters via the on-screen keyboard. The save persists across reboots.

### Stats (the four things to watch)

Your pet has four decaying stats. Anything above 100% is bad — that's "starving / sick / drained / tired". The action menu icons (top row) let you reset each:

| Stat        | Reset by         | Notes                                              |
| ----------- | ---------------- | -------------------------------------------------- |
| Hunger      | **Feed**         | Costs nothing, daily                                |
| Sick        | **Heal**         | Use when the sick icon appears                      |
| Drained     | **Play** or mini-game | Most important — also resets "miserable"       |
| Tired       | **Rest**         | Or **Hibernate** for long sleeps                    |

### Controls

| Key            | Action                                       |
| -------------- | -------------------------------------------- |
| Up / Down      | switch between top row (actions) and bottom row |
| Left / Right   | move along the row                           |
| EXE / Fire     | activate the highlighted icon                |
| CAN            | back out                                     |

### Hibernate

If you're putting the badge away for more than a few hours, open the action menu and **Hibernate**. Stats freeze until you wake the pet. Forget to hibernate before storing the badge and the pet decays — by the time you find it again it may have starved.

## Game modes

Two built-in difficulty presets, picked via **Main → Bornagotchi → Mode**:

- **Classic** — the original balance the badge ships with.
- **Casual** — roughly half the decay speed, doubled lifetimes, more relief per Feed / Heal / Relax action. Friendly for people who don't want to baby-sit.

The setting persists in flash. Changing it shows a `*` next to the mode name in the menu — that means the new mode is queued but the active engine is still running the old one. **Reboot the badge** (unplug + plug USB, or hit the factory-reset combo if you really want to start clean) for it to take effect.

## Custom balance: `BORNPETS.CFG`

If neither preset is your speed you can override individual numbers via a config file on the badge's USB drive.

1. Plug the badge in via USB-C.
2. Open the `CYBR<4 hex>` drive on your computer.
3. Create or edit `BORNPETS.CFG` in the root with one `KEY=VALUE` per line:

   ```
   # speed up hunger decay, slow down drained
   HUNGER_RATE=4
   DRAINED_INTERVAL=180
   ```

4. Eject the drive and reboot the badge.

When a `BORNPETS.CFG` is active, the pet name on the stats screen and traits screen gets a small `*` after it — so you can tell at a glance that your pet is running on a non-standard balance.

Known keys (everything else is silently ignored, so you don't have to keep your file up to date):

| Key | What it does | Reasonable range |
| --- | --- | --- |
| `HUNGER_RATE` | how fast hunger fills per tick | 1 – 30 |
| `TIRED_RATE` | how fast tired fills per tick | 1 – 30 |
| `SLEEP_HUNGER_COST` | hunger gained per sleep tick | 0 – 6000 |
| `DRAINED_INTERVAL` | ticks between drain bumps | 30 – 360 |
| `DRAINED_INTERVAL_MISERABLE` | drain interval when miserable | 10 – 120 |
| `SICK_RATE` | baseline sick decay per tick | 0 – 5 |
| `SICK_CONDITION_RATE` | sick decay when stats are bad | 0 – 2000 |
| `SICK_CONDITION_MISERABLE_RATE` | sick decay when miserable | 0 – 4000 |
| `MISERABLE_INTERVAL_BASE` | base miserable interval | 120 – 600 |
| `MISERABLE_INTERVAL_MIN` | min miserable interval (when many stats are bad) | 10 – 200 |
| `FEED_HUNGER_RELIEF` | hunger drop per Feed tick | 1000 – 20000 |
| `FEED_DRAINED_RELIEF` | drained drop per Feed tick | 0 – 5000 |
| `HEAL_SICK_RELIEF` | sick drop per Heal tick | 1000 – 30000 |
| `RELAX_DRAINED_RELIEF` | drained drop per Relax tick | 1000 – 20000 |
| `RELAX_HUNGER_COST` | hunger gained per Relax tick | 0 – 10000 |
| `PLAY_HUNGER_COST` | hunger gained per Play tick | 0 – 3000 |
| `PLAY_TIRED_COST` | tired gained per Play tick | 0 – 3000 |
| `PLAY_DRAINED_COST` | drained gained per Play tick | 0 – 3000 |
| `MINIGAME_HUNGER_COST` | hunger gained per mini-game completed | 0 – 10000 |
| `MINIGAME_COOLDOWN` | ticks between mini-game stat awards | 1 – 200 |
| `HATCHING_TICKS` | how long hatching takes (6 = 1 minute) | 1 – 200 |
| `MAX_SLEEP_TICKS` | max ticks the engine sleeps between updates | 30 – 600 |

Stat scale: `655 ≈ 1 %`, `65535 = 100 %`. Time scale: `1 tick = 10 seconds` (so 360 ticks = 1 hour). Values are clamped to sensible ranges (interval keys are floored to 1 so the engine can't divide by zero). Unknown keys are logged and skipped.

To go back to a preset, delete `BORNPETS.CFG` from the drive and reboot.

A few gotchas:

- **Edits apply at boot only.** Eject the drive properly (so the write is
  flushed to the badge), then power-cycle. No `*` after the pet name =
  no override was applied.
- **The parser is silent.** Unknown keys are skipped, and a value that
  isn't a plain whole number (no units, decimals or minus sign) drops
  the whole line without any error on screen.
- **The "Reasonable range" column is advice, not a limit.** Values are
  only clamped to the raw integer type — `HUNGER_RATE=1000` really does
  make hunger fill ~300× faster and your pet will be starving before
  you've unplugged the cable. If a wild value wrecked your pet, delete
  the file and reboot to fall back to the preset.

## Custom pets: `PETS.CFG`

You can add your own pets (or rename the built-in ones) without
reflashing — a manifest file plus sprite files on the badge's USB drive.
A pet is a pure cosmetic skin: stats, decay, traits and animations all
behave identically; a new pet only needs art.

This is also how **Panda** — the fourth hatchery option — is shipped:
it's a `PETS.CFG` pet at prefix `5` installed by default, not a
hardcoded built-in. If you write your own `PETS.CFG`, prefix `5`
already means Panda unless you rename or overwrite it; use `6` or `7`
for a pet of your own alongside it.

### The manifest

Create `PETS.CFG` in the root of the `CYBR<4 hex>` drive, one
`PREFIX=NAME` per line (`#` starts a comment):

```
# rename a built-in, add a new pet at the one free slot (5 is Panda by default)
0=Bartho
6=Ghost
```

The prefix (decimal) picks the sprite slot:

| Prefix  | Meaning                                              |
| ------- | ---------------------------------------------------- |
| `0` `1` `2` | The built-ins (Bartholomeus, Cat, Slug) — listing one just **renames** it |
| `3` `4` | Reserved (menu icons, ex-sponsor slideshow) — ignored |
| `5`     | **Panda** by default — a `PETS.CFG` pet like any other, so listing it renames it same as a built-in |
| `6` `7` | **New pets** — appear in the hatchery roster         |
| `8`+    | Out of range — ignored                               |

Names are ASCII only, max 16 characters (longer is truncated, a
non-ASCII character truncates at that point). Malformed lines are
skipped silently. Applied at boot — eject the drive properly and
power-cycle, like every other config file.

### The sprites

Each animation frame is one PCX file named `PPAAFF.PCX`, all three
fields two-digit **hex**:

- `PP` — the pet prefix (`05`, `06`, `07` for new pets)
- `AA` — the animation ID (table below)
- `FF` — the frame number, starting at `00` and contiguous (`00`, `01`,
  `02`, …). Frame counts are auto-discovered; a missing frame `00`
  means "no such animation" and the badge falls back or shows nothing —
  so at minimum ship Idle (`PP0100.PCX`).

| `AA` | Animation        | `AA` | Animation          |
| ---- | ---------------- | ---- | ------------------ |
| `01` | Idle             | `0B` | Warning: miserable |
| `02` | Happy            | `0C` | Feeding            |
| `03` | Critical: sick   | `0D` | Healing            |
| `04` | Critical: tired  | `0E` | Relaxing           |
| `05` | Critical: hungry | `0F` | Playing            |
| `06` | Critical: drained| `10` | Sleeping           |
| `07` | Warning: sick    | `11` | Leaving            |
| `08` | Warning: tired   | `12` | Gone               |
| `09` | Warning: hungry  | `13` | Hibernating        |
| `0A` | Warning: drained | `14` | Hatching (egg)     |

Example: `050100.PCX` = pet 5, Idle, frame 0. `051400.PCX` = pet 5's
egg. Any animation you don't supply simply never plays for that pet.

The files use the same strict PCX flavour as all badge art (2 bpp,
single plane, RLE, fixed palette 0 = black / 1 = red / 2 = white /
3 = transparent) — generate them with the `aegg-asset-assistant` tool.
With three free prefixes (`5`–`7`) the roster tops out at the three
built-ins plus three custom pets.

## The five mini-games

Open the bottom-row **Play** menu in BornPets, then pick a game. Each game has its own cooldown (limit on how often you can play it for stat reduction). **CAN** always exits any mini-game.

| Game            | Goal                                            | Notes                               |
| --------------- | ----------------------------------------------- | ----------------------------------- |
| **Tic-Tac-Toe** | Draw or beat the computer's X-vs-O              | Difficulty: Normal (computer slips 35%) or Impossible (never slips) |
| **Lights Out**  | Toggle squares on a 5×5 grid until all are off  | Toggling a cell flips its 4 neighbours too. All seeds are solvable. |
| **Nim**         | Force the computer to take the last stick       | Four rows of 1 / 3 / 5 / 7 sticks. Two-phase input: pick row, then count. |
| **Black Hole**  | Lower adjacent-sum than the AI                 | 21-cell pyramid, both players place numbers 1..10 in alternating turns. |
| **BornJeweled** | Match-3 with a move limit                       | 6×6 board, 30 moves. Shapes only — no colour pairs, fully accessible. |

All games share these controls inside play:

| Key            | Action                                       |
| -------------- | -------------------------------------------- |
| Up/Down/Left/Right | move cursor                              |
| EXE / Fire     | place / select                               |
| CAN            | quit back to the Play menu                   |

Each win reduces the "drained" stat without raising "hunger" — they're free entertainment.
