# BornPets & Mini-Games — User Guide

The **Game** screen runs BornPets — a virtual pet inspired by Tamagotchi — plus four mini-games you can launch from the pet's **Play** menu.

## BornPets

Hatch a snail or a cat, then keep it fed, healthy, rested and entertained.

### Hatching

The very first time you open Game you see the hatchery. Push **EXE / Fire** to start. Pick **Snail** or **Cat**, then wait about a minute for the egg to hatch. After hatching you name your pet — up to 12 characters via the on-screen keyboard. The save persists across reboots.

### Stats (two pages)

Open the Stats icon and pick **View stats**. **Left / Right** flips between two pages of bars: **Needs** (Full, Water, Energy, Fun, Clean) and **Body** (Health, Fit, Weight, Booze, Mood, plus a line naming the pet's conditions such as "injured" or "hung over"). It always opens on page 1; Fire or CAN goes back.

Each bar has thin ticks marking its comfort band. A black fill between the ticks is fine; a **red** fill means the value is out of its band. The Health bar's extra tick is the ceiling, the most health the pet can recover to right now. Feed, water, play, sleep, wash, exercise and the rest of the action menu are how you bring a bar back into its band.

The **status line** under the pet names the one or two things that need you most ("hungry, bored"; "no insulin" and "needs rehab" when medication or rehab has lapsed; "content" when all is well). It turns red only in a health danger zone, and the badge buzzes only for danger or a real alarm. The corner banners of earlier versions are gone.

### Controls

| Key            | Action                                       |
| -------------- | -------------------------------------------- |
| Up / Down      | switch between top row (actions) and bottom row |
| Left / Right   | move along the row                           |
| EXE / Fire     | activate the highlighted icon                |
| CAN            | back out                                     |

### Hibernate

If you're putting the badge away for more than a few hours, open the action menu and **Hibernate**. Stats freeze until you wake the pet. Forget to hibernate before storing the badge and the pet decays — by the time you find it again it may have starved.

### Sky and floor

The game screen has a background. By day the sky is white with a sun and a small cloud; at night it is a dark dither with a moon and stars, following the badge clock (the night hours are `BG_NIGHT_FROM_MIN` and `BG_NIGHT_TO_MIN` in `BPETS_V3.CFG`). With no clock set the sky stays plain white. The night sky is only the hour, not the pet's bedtime. As dirt rises, items pile up on the floor, one more at each of the `FLOOR_*_DIRT_PCT` lines, and a wash clears them. A poorly pet gets a small red cross. A hibernating pet has no sky. The sky costs no extra screen flashes.

### Demo mode

Demo mode turns the pet into a desk companion that looks after itself. It eats, drinks, washes, sleeps, plays and takes its medicine and insulin on its own, and it says what it did in a short note under the pet ("had an apple", "off to bed"). Its condition wanders but it never gets bad enough to leave, and it never exercises: that stays your job, so a demo pet that gets unfit says "I feel stiff." in the hint. It follows the badge clock for bedtime, or its own rhythm while the clock is not set. Demo pets keep a small black **D** under the battery bar on the game screen. HAX keeps running while demo is on: the pet pays for its own meals and medicine, earns the money back (it plays its mini-games and goes to work, "did a shift", whenever the balance is under `DEMO_HAX_TARGET`) and the balance drifts down by 1 HAX every `DEMO_HAX_DRIFT_MIN` minutes while it is above the target, so it stays around 100 HAX. Turning demo off keeps whatever balance there is.

You can step in at any time. After you act, the pet leaves its care to you for 30 minutes and thanks you ("thanks!") when your action helped. When its health drops to 35 % it shows "could use a hand" instead of a note. Demo is silent: no buzzer.

Both switches are in **Main -> Bornagotchi**, under **Mode**:

- **Demo mode: on / off** turns it on or off. A pet in poor health (under 40 %) cannot start it and the row says "Demo: fix me first"; an egg or a pet that left shows "Demo: no pet".
- **Demo care: instant / animated** picks how the pet looks after itself. Instant (the default) applies the care at once and leaves a note. Animated plays the real eat, drink and wash animations; it costs more screen refreshes, so it drains the battery faster. It is saved per pet and can be set before turning demo on.

### Battles

Pets can fight the friends they have met over the mesh radio. Open the **Play** menu and pick **Battle: <form>**. The word after the colon (`top form`, `in form`, `off form` or `poor form`) says how your pet would do against an average pet. The list shows each friend with `~<n>%`, an estimate of your chance from their last beacon, and how old that beacon is. Fire sends the challenge.

The other badge buzzes and shows a card with both pets' numbers (power, guard, speed, stamina, focus, HP) and the exact chance. Its player has 15 seconds to choose **Fire** (fight) or **Cancel** (decline). You wait up to 20 seconds. If nobody answers, the fight is played against the friend's last beacon and the result card shows as usual; with no usable beacon the card says `no answer` and nothing happens.

The fight is ten rounds at most. The faster pet strikes first, a pet that is past its stamina slows down, and after ten rounds the pet with more HP left wins. Care decides the numbers: a rested, fed, clean, cheerful pet fights well. Twenty words (such as `well rested`, `hung over`, `sore`) show what helps or hurts. The result card shows `chance was <n>%` and what decided the fight.

A fight costs energy, water and dirt, and adds strain and bruises. A tired, hungry or thirsty pet strains more, and too much strain tears a muscle. In money mode the first win of the day pays `BATTLE_PRIZE_FIRST_HAX` and later wins `BATTLE_PRIZE_LATER_HAX`; a fight against a beacon older than `BATTLE_PRIZE_BEACON_MIN` minutes pays nothing and costs nothing, but still counts in the record and sets the Battle rest. Declining costs a little mood (`BATTLE_DECLINE_MOOD_PCT` for `BATTLE_DECLINE_MIN` minutes). Demo pets fight only demo pets. The balance numbers are keys in `BPETS_V3.CFG` (see the key list below).

## Game modes

Two built-in difficulty presets, picked via **Main → Bornagotchi → Mode**:

- **Classic** — the original balance the badge ships with.
- **Casual** — roughly half the decay speed, doubled lifetimes, more relief per Feed / Heal / Relax action. Friendly for people who don't want to baby-sit.

The setting persists in flash. Changing it shows a `*` next to the mode name in the menu — that means the new mode is queued but the active engine is still running the old one. **Reboot the badge** (unplug + plug USB, or hit the factory-reset combo if you really want to start clean) for it to take effect.

## Custom balance: `BPETS_V3.CFG`

If neither preset is your speed you can override individual numbers via a config file on the badge's USB drive. The BornPets v3 firmware reads `BPETS_V3.CFG`. An old `BORNPETS.CFG` is left alone for the old firmware, so you can go back to the old firmware and keep your old settings. Updating the badge's assets never overwrites an existing `BPETS_V3.CFG`.

1. Plug the badge in via USB-C.
2. Open the `CYBR<4 hex>` drive on your computer.
3. Create or edit `BPETS_V3.CFG` in the root with one `KEY=VALUE` per line:

   ```
   # halve the starvation damage, make the mood move faster
   DMG_FULL_LOW_PM=20
   MOOD_STEP=200
   ```

   `BPETS_V3.CFG` already lists every key as a comment, with the Classic value and the Casual value. To use a line, remove the `# ` at the start and set your number. The `# Casual ...` note at the end is a comment and can stay.

4. Eject the drive and reboot the badge.

When a `BPETS_V3.CFG` changes at least one value, the pet name on the stats screen and traits screen gets a small `*` after it — so you can tell at a glance that your pet is running on a non-standard balance. The template as shipped has no active lines, so it changes nothing.

Known keys (everything else is silently ignored, so you don't have to keep your file up to date). Name endings: `_PCT` is a percent of a stat, `_MIN` is minutes, `_PM` is stat units per minute (`655` = 1 %).

| Key | Classic | Casual |
| --- | --- | --- |
| `MINIGAME_COOLDOWN` | 18 | 12 |
| `HATCHING_TICKS` | 1 | 1 |
| `MAX_SLEEP_TICKS` | 720 | 720 |
| `FULL_LO_PCT` | 38 | 33 |
| `FULL_HI_PCT` | 78 | 83 |
| `WATER_LO_PCT` | 38 | 33 |
| `WATER_HI_PCT` | 83 | 88 |
| `ENERGY_LO_PCT` | 33 | 28 |
| `FUN_LO_PCT` | 33 | 28 |
| `FUN_HI_PCT` | 73 | 78 |
| `DIRT_HI_PCT` | 58 | 63 |
| `BOOZE_HI_PCT` | 38 | 43 |
| `BOOZE_DANGER_PCT` | 70 | 75 |
| `STRAIN_INJ_PCT` | 60 | 65 |
| `FIT_LO_PCT` | 43 | 38 |
| `WEIGHT_LO_PCT` | 38 | 33 |
| `WEIGHT_HI_PCT` | 63 | 68 |
| `FULL_DRAIN_AWAKE_PM` | 54 | 54 |
| `WATER_DRAIN_AWAKE_PM` | 45 | 45 |
| `ENERGY_DRAIN_AWAKE_PM` | 36 | 36 |
| `FUN_DRAIN_PM` | 40 | 40 |
| `FULL_DRAIN_ASLEEP_PM` | 30 | 30 |
| `WATER_DRAIN_ASLEEP_PM` | 20 | 20 |
| `ENERGY_GAIN_ASLEEP_PM` | 120 | 120 |
| `SLEEP_FUN_FLOOR_PCT` | 40 | 40 |
| `DIRT_GAIN_PM` | 8 | 8 |
| `BOOZE_CLEAR_PM` | 36 | 36 |
| `BOOZE_CLEAR_SLOW_PCT` | 67 | 67 |
| `STRAIN_CLEAR_PM` | 80 | 80 |
| `FIT_DECAY_HIGH_PM` | 4 | 4 |
| `FIT_DECAY_MID_PM` | 2 | 2 |
| `FIT_DECAY_LOW_PM` | 1 | 1 |
| `FIT_DECAY_HIGH_ABOVE_PCT` | 50 | 50 |
| `FIT_DECAY_MID_ABOVE_PCT` | 25 | 25 |
| `FIT_DECAY_HEAVY_PCT` | 150 | 150 |
| `WEIGHT_PULL_HEAVY_PM` | 3 | 3 |
| `WEIGHT_PULL_PM` | 1 | 1 |
| `WEIGHT_BELLY_PM` | 3 | 3 |
| `ENERGY_MULT_UNFIT_PCT` | 125 | 125 |
| `ENERGY_MULT_FIT_PCT` | 90 | 90 |
| `ENERGY_FIT_GOOD_PCT` | 70 | 70 |
| `ENERGY_MULT_DRUNK_PCT` | 150 | 150 |
| `ENERGY_MULT_WIRED_PCT` | 150 | 150 |
| `ENERGY_MULT_HANGOVER_PCT` | 125 | 125 |
| `ENERGY_MULT_WEIGHT_PCT` | 115 | 115 |
| `ENERGY_CAP_DEPRIVED_PCT` | 70 | 70 |
| `THIRST_MULT_BASE_PCT` | 100 | 100 |
| `THIRST_MULT_LOW_PCT` | 110 | 110 |
| `THIRST_MULT_MID_PCT` | 125 | 125 |
| `THIRST_MULT_HIGH_PCT` | 140 | 140 |
| `THIRST_BOOZE_LOW_PCT` | 15 | 15 |
| `THIRST_BOOZE_MID_PCT` | 40 | 40 |
| `THIRST_BOOZE_HIGH_PCT` | 70 | 70 |
| `SLEEP_QUALITY_POOR_PM` | 600 | 600 |
| `SLEEP_QUALITY_GOOD_PM` | 1000 | 1000 |
| `UNSETTLED_BOOZE_PCT` | 30 | 30 |
| `UNSETTLED_FULL_LO_PCT` | 25 | 25 |
| `UNSETTLED_FULL_HI_PCT` | 85 | 85 |
| `UNSETTLED_WATER_LO_PCT` | 25 | 25 |
| `UNSETTLED_WATER_HI_PCT` | 90 | 90 |
| `MOOD_STEP` | 120 | 120 |
| `MOOD_PEN_INJURED_PCT` | 20 | 20 |
| `MOOD_PEN_GROGGY_PCT` | 10 | 10 |
| `MOOD_PEN_HANGOVER_PCT` | 15 | 15 |
| `MOOD_PEN_DEPRIVED_PCT` | 10 | 10 |
| `MOOD_PEN_REHAB_PCT` | 20 | 20 |
| `MOOD_PEN_CRAVING_PCT` | 15 | 15 |
| `MOOD_CRAVING_BOOZE_PCT` | 10 | 10 |
| `MOOD_HEALTH_FLOOR_PCT` | 50 | 50 |
| `CEILING_FIT_STEP1_PCT` | 25 | 25 |
| `CEILING_FIT_STEP2_PCT` | 50 | 50 |
| `CEILING_FIT_STEP3_PCT` | 70 | 70 |
| `CEILING_STEP0_PCT` | 55 | 55 |
| `CEILING_STEP1_PCT` | 65 | 65 |
| `CEILING_STEP2_PCT` | 80 | 80 |
| `CEILING_STEP3_PCT` | 100 | 100 |
| `CEILING_DIABETIC_PCT` | 20 | 20 |
| `DMG_NEED_LOW_PCT` | 15 | 15 |
| `DMG_NEED_HIGH_PCT` | 95 | 95 |
| `DMG_ENERGY_PCT` | 10 | 10 |
| `DMG_ENERGY_HEALTH_PCT` | 30 | 30 |
| `DMG_MOOD_PCT` | 20 | 20 |
| `DMG_DIRT_PCT` | 85 | 85 |
| `DMG_WEIGHT_HIGH_PCT` | 80 | 80 |
| `DMG_WEIGHT_LOW_PCT` | 20 | 20 |
| `DMG_FULL_LOW_PM` | 40 | 20 |
| `DMG_FULL_HIGH_PM` | 8 | 4 |
| `DMG_WATER_LOW_PM` | 40 | 20 |
| `DMG_WATER_HIGH_PM` | 8 | 4 |
| `DMG_ENERGY_PM` | 8 | 4 |
| `DMG_BOOZE_PM` | 40 | 20 |
| `DMG_MOOD_PM` | 10 | 5 |
| `DMG_DIRT_PM` | 10 | 5 |
| `DMG_WEIGHT_HIGH_PM` | 2 | 1 |
| `DMG_WEIGHT_LOW_PM` | 10 | 5 |
| `REC_GOOD_PM` | 14 | 14 |
| `REC_FAIR_PM` | 5 | 5 |
| `REC_GOOD_MOOD_PCT` | 60 | 60 |
| `REC_FAIR_MOOD_PCT` | 40 | 40 |
| `REC_DEPRIVED_PCT` | 50 | 50 |
| `DEPENDENCE_ONSET_MIN` | 1200 | 1200 |
| `DEPENDENCE_RELAPSE_MIN` | 400 | 400 |
| `DIABETES_ONSET_MIN` | 4320 | 4320 |
| `DIABETES_REVERSAL_MIN` | 10080 | 10080 |
| `HANGOVER_MIN` | 480 | 480 |
| `RECOVERING_MIN` | 10080 | 10080 |
| `SELF_WAKE_MIN` | 720 | 720 |
| `OVERSLEEP_WAKE_MIN` | 300 | 300 |
| `GROGGY_LINE_MIN` | 240 | 240 |
| `GROGGY_MIN` | 120 | 120 |
| `GROGGY_ENERGY_PCT` | 10 | 10 |
| `GROGGY_FIT_PCT` | 2 | 2 |
| `RESTED_MIN` | 960 | 960 |
| `GOOD_SLEEP_LINE_MIN` | 300 | 300 |
| `NIGHT_SHORT_MIN` | 360 | 360 |
| `NIGHT_GOOD_MIN` | 390 | 390 |
| `DEPRIVED_NIGHTS` | 3 | 3 |
| `CLEARING_NIGHTS` | 2 | 2 |
| `GRUMPY_PCT` | 10 | 10 |
| `GRUMPY_ENERGY_PCT` | 50 | 50 |
| `COLLAPSE_DAMAGE` | 1965 | 982 |
| `COLLAPSE_HEALTH_FLOOR_PCT` | 33 | 33 |
| `LEAVING_MIN` | 720 | 720 |
| `BOOZE_CLEAR_PCT` | 15 | 15 |
| `DIABETES_WEIGHT_HI_PCT` | 80 | 80 |
| `DIABETES_WEIGHT_LO_PCT` | 65 | 65 |
| `REVERSAL_FIT_PCT` | 60 | 60 |
| `HANGOVER_ASLEEP_MULT` | 2 | 2 |
| `HEALTH_EXCESS_DECAY_PM` | 3 | 3 |
| `CARE_GOOD_MOOD_PCT` | 60 | 60 |
| `OVERSLEEP_RESET_PCT` | 85 | 85 |
| `DOSE_SALAD_FULL_PCT` | 20 | 20 |
| `DOSE_APPLE_FULL_PCT` | 25 | 25 |
| `DOSE_FRIKANDEL_FULL_PCT` | 35 | 35 |
| `DOSE_PIZZA_FULL_PCT` | 40 | 40 |
| `DOSE_CAKE_FULL_PCT` | 10 | 10 |
| `DOSE_SALAD_DIRT_PCT` | 10 | 10 |
| `DOSE_APPLE_DIRT_PCT` | 10 | 10 |
| `DOSE_FRIKANDEL_DIRT_PCT` | 10 | 10 |
| `DOSE_PIZZA_DIRT_PCT` | 10 | 10 |
| `DOSE_CAKE_DIRT_PCT` | 5 | 5 |
| `DOSE_SALAD_WEIGHT_PM` | 0 | 0 |
| `DOSE_APPLE_WEIGHT_PM` | 5 | 5 |
| `DOSE_FRIKANDEL_WEIGHT_PM` | 20 | 20 |
| `DOSE_PIZZA_WEIGHT_PM` | 20 | 20 |
| `DOSE_CAKE_WEIGHT_PM` | 30 | 30 |
| `DOSE_FRIKANDEL_MOOD_PCT` | 6 | 6 |
| `DOSE_PIZZA_MOOD_PCT` | 8 | 8 |
| `DOSE_CAKE_MOOD_PCT` | 15 | 15 |
| `DOSE_HEALTHY_WATER_PCT` | 8 | 8 |
| `DOSE_SALAD_FIT_PCT` | 1 | 1 |
| `DOSE_JUNK_REPEAT_DIV` | 3 | 3 |
| `DOSE_JUNK_MIN` | 180 | 180 |
| `DOSE_FULL_CAP_PCT` | 120 | 120 |
| `DOSE_EXCESS_ENERGY_DIV` | 2 | 2 |
| `DOSE_EXCESS_WEIGHT_DIV` | 4 | 4 |
| `DOSE_WATER_MAX_PCT` | 30 | 30 |
| `DOSE_WATER_BELLY_PCT` | 2 | 2 |
| `DOSE_WATER_BELLY_MIN_PCT` | 15 | 15 |
| `DOSE_WATER_BOOZE_PCT` | 5 | 5 |
| `DOSE_WATER_HANGOVER_MIN` | 60 | 60 |
| `DOSE_WATER_GROGGY_MIN` | 30 | 30 |
| `DOSE_COFFEE_WATER_PCT` | 20 | 20 |
| `DOSE_COFFEE_FULL_PCT` | 2 | 2 |
| `DOSE_COFFEE_ENERGY_PCT` | 8 | 8 |
| `DOSE_COFFEE_MOOD_PCT` | 3 | 3 |
| `DOSE_COFFEE_WEIGHT_PCT` | 0 | 0 |
| `COFFEE_WINDOW_MIN` | 240 | 240 |
| `COFFEE_WIRED_CUPS` | 3 | 3 |
| `COFFEE_WIRED_MIN` | 120 | 120 |
| `DOSE_BEER_WATER_PCT` | 20 | 20 |
| `DOSE_BEER_BELLY_PCT` | 2 | 2 |
| `DOSE_BEER_BOOZE_PCT` | 15 | 15 |
| `DOSE_BEER_MOOD_PCT` | 12 | 12 |
| `DOSE_WINE_WATER_PCT` | 8 | 8 |
| `DOSE_WINE_BELLY_PCT` | 1 | 1 |
| `DOSE_WINE_BOOZE_PCT` | 20 | 20 |
| `DOSE_WINE_MOOD_PCT` | 15 | 15 |
| `DOSE_WHISKEY_WATER_PCT` | 0 | 0 |
| `DOSE_WHISKEY_BELLY_PCT` | 0 | 0 |
| `DOSE_WHISKEY_BOOZE_PCT` | 30 | 30 |
| `DOSE_WHISKEY_MOOD_PCT` | 20 | 20 |
| `DOSE_BEER_WEIGHT_PCT` | 1 | 1 |
| `DOSE_ALCOHOL_ENERGY_PCT` | 5 | 5 |
| `DOSE_ALCOHOL_FUN_PCT` | 5 | 5 |
| `DOSE_TIRED_BOOZE_PCT` | 150 | 150 |
| `DOSE_DEPENDENT_DIV` | 2 | 2 |
| `DOSE_RELAPSE_MOOD_PCT` | 20 | 20 |
| `DOSE_PLAY_FUN_PCT` | 25 | 25 |
| `DOSE_PLAY_ENERGY_PCT` | 5 | 5 |
| `DOSE_PLAY_FULL_PCT` | 3 | 3 |
| `DOSE_PLAY_WATER_PCT` | 4 | 4 |
| `DOSE_PLAY_FIT_PCT` | 2 | 2 |
| `DOSE_PLAY_DIRT_PCT` | 3 | 3 |
| `DOSE_PLAY_GROGGY_MIN` | 60 | 60 |
| `DOSE_TIRED_FUN_MULT` | 2 | 2 |
| `DOSE_MINI_FUN_BASE_PCT` | 10 | 10 |
| `DOSE_MINI_FUN_GRADE_PCT` | 7 | 7 |
| `DOSE_MINI_ENERGY_PCT` | 3 | 3 |
| `DOSE_EXERCISE_STRAIN_PCT` | 35 | 35 |
| `DOSE_STRAIN_NEED_PCT` | 150 | 150 |
| `DOSE_STRAIN_RESTED_PCT` | 70 | 70 |
| `DOSE_STRAIN_FIT_PCT` | 85 | 85 |
| `DOSE_STRAIN_FIT_MIN_PCT` | 70 | 70 |
| `DOSE_STRAIN_HEAVY_PCT` | 120 | 120 |
| `DOSE_STRAIN_DEPRIVED_PCT` | 125 | 125 |
| `DOSE_EXERCISE_ENERGY_PCT` | 10 | 10 |
| `DOSE_EXERCISE_FULL_PCT` | 8 | 8 |
| `DOSE_EXERCISE_WATER_PCT` | 12 | 12 |
| `DOSE_EXERCISE_FUN_PCT` | 10 | 10 |
| `DOSE_EXERCISE_BOOZE_PCT` | 5 | 5 |
| `DOSE_EXERCISE_DIRT_PCT` | 12 | 12 |
| `DOSE_EXERCISE_WEIGHT_PCT` | 3 | 3 |
| `DOSE_EXERCISE_HANGOVER_MIN` | 120 | 120 |
| `DOSE_EXERCISE_FIT_PCT` | 12 | 12 |
| `DOSE_INJURY_MIN` | 1440 | 1440 |
| `DOSE_INJURY_HEALTH_PCT` | 5 | 5 |
| `DOSE_WASH_DIRT_PCT` | 70 | 70 |
| `DOSE_RELAX_FUN_PCT` | 25 | 25 |
| `DOSE_RELAX_STRAIN_PCT` | 15 | 15 |
| `DOSE_RELAX_ENERGY_PCT` | 3 | 3 |
| `DOSE_RELAX_FIT_PCT` | 1 | 1 |
| `DOSE_MED_CD_MIN` | 240 | 240 |
| `DOSE_HEAL_INJURY_MIN` | 720 | 720 |
| `DOSE_MED_BOOZE_PCT` | 10 | 10 |
| `DOSE_HEAL_HEALTHY_PCT` | 80 | 80 |
| `DOSE_HEAL_HEALTH_PCT` | 10 | 10 |
| `DOSE_INSULIN_WINDOW_MIN` | 120 | 120 |
| `DOSE_INSULIN_MIN` | 480 | 480 |
| `DOSE_OZEMPIC_WEIGHT_PCT` | 15 | 15 |
| `DOSE_OZEMPIC_FIT_PCT` | 5 | 5 |
| `DOSE_OZEMPIC_MIN` | 1440 | 1440 |
| `DOSE_OZEMPIC_CD_MIN` | 2880 | 2880 |
| `DOSE_REHAB_MIN` | 4320 | 4320 |
| `DOSE_NIGHT_ENERGY_PCT` | 50 | 50 |
| `DOSE_WORK_ENERGY_PCT` | 15 | 15 |
| `DOSE_WORK_FUN_PCT` | 10 | 10 |
| `DOSE_WORK_MOOD_PCT` | 5 | 5 |
| `DOSE_FRIEND_BIG_FUN_PCT` | 15 | 15 |
| `DOSE_FRIEND_SMALL_FUN_PCT` | 5 | 5 |
| `DEMO_FIT_FLOOR_PCT` | 45 | 40 |
| `DEMO_HEALTH_FLOOR_PCT` | 25 | 25 |
| `DEMO_HELP_HEALTH_PCT` | 35 | 35 |
| `DEMO_ON_HEALTH_PCT` | 40 | 40 |
| `DEMO_MED_HEALTH_PCT` | 45 | 45 |
| `DEMO_WAKE_FULL_PCT` | 18 | 18 |
| `DEMO_EDGE_FULL_PCT` | 20 | 20 |
| `DEMO_EDGE_WATER_PCT` | 22 | 22 |
| `DEMO_EDGE_ENERGY_PCT` | 12 | 12 |
| `DEMO_EDGE_BOOZE_PCT` | 55 | 55 |
| `DEMO_SOFT_WATER_PCT` | 39 | 34 |
| `DEMO_SOFT_FULL_PCT` | 27 | 22 |
| `DEMO_SOFT_ENERGY_PCT` | 27 | 22 |
| `DEMO_SOFT_DIRT_PCT` | 68 | 73 |
| `DEMO_SOFT_FUN_PCT` | 15 | 10 |
| `DEMO_PLAY_ENERGY_PCT` | 25 | 25 |
| `DEMO_WIRED_FUN_PCT` | 78 | 83 |
| `DEMO_RISE_ENERGY_PCT` | 97 | 97 |
| `DEMO_NAP_ENERGY_PCT` | 55 | 55 |
| `DEMO_NAP_FUN_PCT` | 45 | 45 |
| `DEMO_WHIM_BOOZE_LO_PCT` | 15 | 15 |
| `DEMO_WHIM_BOOZE_HI_PCT` | 45 | 45 |
| `DEMO_WHIM_BEER_LO_PCT` | 50 | 50 |
| `DEMO_WHIM_BEER_HI_PCT` | 35 | 35 |
| `DEMO_BEER_STOP_MIN` | 300 | 300 |
| `DEMO_LIGHT_FOOD_PCT` | 56 | 56 |
| `DEMO_RICH_FOOD_PCT` | 42 | 42 |
| `DEMO_REACT_BASE_MIN` | 30 | 30 |
| `DEMO_REACT_SPREAD_MIN` | 80 | 80 |
| `DEMO_DRY_BASE_MIN` | 20 | 20 |
| `DEMO_DRY_SPREAD_MIN` | 40 | 40 |
| `DEMO_WHIM_BASE_MIN` | 180 | 180 |
| `DEMO_WHIM_SPREAD_MIN` | 600 | 600 |
| `DEMO_LAZY_MIN` | 360 | 360 |
| `DEMO_LAZY_MAX_PCT` | 15 | 15 |
| `DEMO_NAP_BASE_MIN` | 40 | 40 |
| `DEMO_NAP_SPREAD_MIN` | 60 | 60 |
| `DEMO_EDGE_NAP_MIN` | 120 | 120 |
| `DEMO_RISE_BASE_MIN` | 10 | 10 |
| `DEMO_RISE_SPREAD_MIN` | 110 | 110 |
| `DEMO_BED_AT_MIN` | 1320 | 1320 |
| `DEMO_BED_SPREAD_MIN` | 90 | 90 |
| `DEMO_UP_AT_MIN` | 420 | 420 |
| `DEMO_UP_SPREAD_MIN` | 90 | 90 |
| `DEMO_PAUSE_MIN` | 30 | 30 |
| `DEMO_PICK_APPLE_PCT` | 40 | 40 |
| `DEMO_PICK_SALAD_PCT` | 15 | 15 |
| `DEMO_PICK_PIZZA_PCT` | 25 | 25 |
| `DEMO_PICK_FRIK_PCT` | 12 | 12 |
| `DEMO_PICK_WATER_PCT` | 70 | 70 |
| `DEMO_PICK_COFFEE_PCT` | 20 | 20 |
| `DEMO_PICK_GAME_PCT` | 30 | 30 |
| `DEMO_HAX_TARGET` | 100 | 100 |
| `DEMO_HAX_DRIFT_MIN` | 10 | 10 |
| `BATTLE_POWER_BASE_PTS` | 22 | 22 |
| `BATTLE_POWER_WEIGHT_PCT` | 28 | 28 |
| `BATTLE_COURAGE_PEAK_PTS` | 8 | 8 |
| `BATTLE_HIGH_MOOD_PCT` | 80 | 80 |
| `BATTLE_HIGH_MOOD_PTS` | 3 | 3 |
| `BATTLE_LOW_MOOD_PCT` | 40 | 40 |
| `BATTLE_GUARD_WEIGHT_PCT` | 26 | 26 |
| `BATTLE_GUARD_BASE_PTS` | 7 | 7 |
| `BATTLE_SPEED_BASE_PTS` | 25 | 25 |
| `BATTLE_SPEED_FIT_PCT` | 40 | 40 |
| `BATTLE_RESTED_SPEED_PTS` | 5 | 5 |
| `BATTLE_RESTED_FOCUS_PTS` | 5 | 5 |
| `BATTLE_GROGGY_SPEED_PTS` | 10 | 10 |
| `BATTLE_GROGGY_FOCUS_PTS` | 5 | 5 |
| `BATTLE_HANGOVER_SPEED_PTS` | 8 | 8 |
| `BATTLE_HANGOVER_FOCUS_PTS` | 12 | 12 |
| `BATTLE_CRAVING_FOCUS_PTS` | 10 | 10 |
| `BATTLE_CRAVING_POWER_PTS` | 3 | 3 |
| `BATTLE_HEAVY_UNFIT_FIT_PCT` | 50 | 50 |
| `BATTLE_SLOPPY_PCT` | 40 | 40 |
| `BATTLE_FELT_BASE_PCT` | 150 | 150 |
| `BATTLE_HP_BASE_PTS` | 45 | 45 |
| `BATTLE_HP_WEIGHT_PCT` | 25 | 25 |
| `BATTLE_POWER_FLOOR_PTS` | 33 | 33 |
| `BATTLE_GUARD_FLOOR_PTS` | 14 | 14 |
| `BATTLE_HP_FLOOR_PTS` | 60 | 60 |
| `BATTLE_STAMINA_FLOOR_PTS` | 1 | 1 |
| `BATTLE_INJURED_PCT` | 60 | 60 |
| `BATTLE_FORM_TOP_PCT` | 65 | 65 |
| `BATTLE_FORM_IN_PCT` | 50 | 50 |
| `BATTLE_FORM_OFF_PCT` | 30 | 30 |
| `BATTLE_COOLDOWN` | 180 | 180 |
| `BATTLE_PAIR_COOLDOWN_MIN` | 60 | 60 |
| `BATTLE_ENERGY_PCT` | 10 | 10 |
| `BATTLE_WATER_PCT` | 8 | 8 |
| `BATTLE_FUN_PCT` | 20 | 20 |
| `BATTLE_DIRT_PCT` | 8 | 8 |
| `BATTLE_STRAIN_PCT` | 20 | 20 |
| `BATTLE_STRAIN_NEED_PCT` | 150 | 150 |
| `BATTLE_STRAIN_RESTED_PCT` | 70 | 70 |
| `BATTLE_BRUISE_MAX_PCT` | 5 | 2 |
| `BATTLE_MOOD_PCT` | 8 | 8 |
| `BATTLE_MIN_HEALTH_PCT` | 30 | 30 |
| `BATTLE_DRUNK_PCT` | 70 | 70 |
| `BATTLE_PRIZE_FIRST_HAX` | 100 | 100 |
| `BATTLE_PRIZE_LATER_HAX` | 20 | 20 |
| `BATTLE_PRIZE_BEACON_MIN` | 30 | 30 |
| `BATTLE_DECLINE_MOOD_PCT` | 3 | 3 |
| `BATTLE_DECLINE_MIN` | 60 | 60 |
| `BG_NIGHT_FROM_MIN` | 1260 | 1260 |
| `BG_NIGHT_TO_MIN` | 420 | 420 |
| `FLOOR_CRUMBS_DIRT_PCT` | 40 | 40 |
| `FLOOR_PEEL_DIRT_PCT` | 55 | 55 |
| `FLOOR_CAN_DIRT_PCT` | 70 | 70 |
| `FLOOR_SOCK_DIRT_PCT` | 85 | 85 |
| `FLOOR_PIZZA_DIRT_PCT` | 95 | 95 |

Stat scale: `655 ≈ 1 %`, `65535 = 100 %`. Time scale: `1 tick = 10 seconds` (so 360 ticks = 1 hour). Values are clamped to sensible ranges. If a pair of band edges ends up in the wrong order, that pair falls back to the preset. The five `FLOOR_*_DIRT_PCT` lines must rise in order, and `BG_NIGHT_FROM_MIN` and `BG_NIGHT_TO_MIN` must differ: if not, all five floor lines (or both night minutes) fall back to the preset together. Unknown keys are logged and skipped. The old cola keys (`DOSE_COLA_*`, `DEMO_PICK_COLA_PCT`) from before cola became coffee still work: they set the matching `DOSE_COFFEE_*` / `DEMO_PICK_COFFEE_PCT` value.

To go back to a preset, delete `BPETS_V3.CFG` from the drive and reboot.

A few gotchas:

- **Edits apply at boot only.** Eject the drive properly (so the write is
  flushed to the badge), then power-cycle. No `*` after the pet name =
  no override was applied.
- **The parser is silent.** Unknown keys are skipped, and a value that
  isn't a plain whole number (no units, decimals or minus sign) drops
  the whole line without any error on screen.
- **Wild values are allowed.** Values are only clamped to the raw
  integer type, so a very large drain rate really will starve your pet
  before you've unplugged the cable. If a wild value wrecked your pet,
  delete the file and reboot to fall back to the preset.

## Custom pets: `PETS.CFG`

You can add your own pets (or rename the built-in ones) without
reflashing — a manifest file plus sprite files on the badge's USB drive.
A pet is a pure cosmetic skin: stats, decay, traits and animations all
behave identically; a new pet only needs art.

### The manifest

Create `PETS.CFG` in the root of the `CYBR<4 hex>` drive, one
`PREFIX=NAME` per line (`#` starts a comment):

```
# rename a built-in, add two new pets
0=Bartho
5=Dragon
6=Ghost
```

The prefix (decimal) picks the sprite slot:

| Prefix  | Meaning                                              |
| ------- | ---------------------------------------------------- |
| `0` `1` `2` | The built-ins (Bartholomeus, Cat, Slug) — listing one just **renames** it |
| `3` `4` | Reserved (sponsor slides, menu icons) — ignored      |
| `5` `6` `7` | **New pets** — appear in the hatchery roster     |
| `8`     | Free pet slot (the Jellyfish used it; no shipped pet) |
| `9`+    | Out of range — ignored                               |

The Jellyfish (prefix `8`) has left the shipped roster. An old badge whose `PETS.CFG` still lists `8=Jellyfish` keeps that name for the Unicorn Realm. With no `08` sprites the Jellyfish cannot be picked, and a live one moves to the Realm.

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

## The four mini-games

Open the bottom-row **Play** menu in BornPets, then pick a game. Each game has its own cooldown (limit on how often you can play it for stat reduction). **CAN** always exits any mini-game.

| Game            | Goal                                            | Notes                               |
| --------------- | ----------------------------------------------- | ----------------------------------- |
| **Tic-Tac-Toe** | Draw or beat the computer's X-vs-O              | Difficulty: Normal (computer slips 35%) or Impossible (never slips) |
| **Lights Out**  | Toggle squares on a 5×5 grid until all are off  | Toggling a cell flips its 4 neighbours too. All seeds are solvable. |
| **Black Hole**  | Lower adjacent-sum than the AI                 | 21-cell pyramid, both players place numbers 1..10 in alternating turns. |
| **BornJeweled** | Match-3 with a move limit                       | 6×6 board, 30 moves. Shapes only — no colour pairs, fully accessible. |

All games share these controls inside play:

| Key            | Action                                       |
| -------------- | -------------------------------------------- |
| Up/Down/Left/Right | move cursor                              |
| EXE / Fire     | place / select                               |
| CAN            | quit back to the Play menu                   |

Each win reduces the "drained" stat without raising "hunger" — they're free entertainment.
