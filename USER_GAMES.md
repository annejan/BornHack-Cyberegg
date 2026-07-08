# BornPets & Mini-Games — User Guide

The **Game** screen runs BornPets — a virtual pet inspired by Tamagotchi — plus seven mini-games you can launch from the pet's **Play** menu.

## BornPets

Hatch a snail or a cat, then keep it fed, healthy, rested and entertained.

### Hatching

The very first time you open Game you see the hatchery. Push **EXE / Fire** to start. Pick **Snail** or **Cat**, then wait about a minute for the egg to hatch. After hatching you name your pet — up to 12 characters via the on-screen keyboard. The save persists across reboots.

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

## The seven mini-games

Open the bottom-row **Play** menu in BornPets, then pick a game. Each game has its own cooldown (limit on how often you can play it for stat reduction). **CAN** always exits any mini-game.

| Game            | Goal                                            | Notes                               |
| --------------- | ----------------------------------------------- | ----------------------------------- |
| **Tic-Tac-Toe** | Draw or beat the computer's X-vs-O              | Difficulty: Normal (computer slips 35%) or Impossible (never slips) |
| **Lights Out**  | Toggle squares on a 5×5 grid until all are off  | Toggling a cell flips its 4 neighbours too. All seeds are solvable. |
| **Nim**         | Force the computer to take the last stick       | Four rows of 1 / 3 / 5 / 7 sticks. Two-phase input: pick row, then count. |
| **Maze**        | Reach any border exit                           | 18×18 maze, visited cells stay shaded. |
| **Black Hole**  | Lower adjacent-sum than the AI                 | 21-cell pyramid, both players place numbers 1..10 in alternating turns. |
| **Triple Born** | Triple Town reskin                              | 6×6 board. Merge three of a kind → next tier. **EXE** swaps the falling piece with the stash. |
| **BornJeweled** | Match-3 with a move limit                       | 6×6 board, 30 moves. Shapes only — no colour pairs, fully accessible. |

All games share these controls inside play:

| Key            | Action                                       |
| -------------- | -------------------------------------------- |
| Up/Down/Left/Right | move cursor                              |
| EXE / Fire     | place / select                               |
| CAN            | quit back to the Play menu                   |

Each win reduces the "drained" stat without raising "hunger" — they're free entertainment.

## Tokens

The **Tokens** screen collects tokens you receive via NFC taps or over the mesh (channel or direct message). Every distinct token is kept in a scrollable list — up to 16 — that stays until you reboot the badge; duplicates are ignored. Long tokens wrap onto several lines (continuation lines are indented) so the whole value is readable. Use **Up/Down** to scroll; **Left/Right** switch screens as usual. See [USER_NFC.md](USER_NFC.md) for how to earn them.
