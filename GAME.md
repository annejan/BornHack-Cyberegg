# BornPets — How to Play

The CyberÆgg badge comes with a virtual pet game. Take care of your pet,
keep it happy, and play mini-games to earn rewards.

## Getting Started

When you first navigate to the game screen, you'll see the start screen.
Press **Fire** to begin — you'll be asked to choose your pet:

- **Snail** — the original CyberÆgg companion
- **Cat** — a feline friend

Use **Up/Down** to highlight your choice and **Fire** to confirm. A
1-minute egg hatching countdown begins with an animation — once it
completes, your pet is born and you'll be asked to give it a name!

Your game is automatically saved to flash. If the badge resets or loses
power, your pet will be right where you left it.

## The Game Screen

```text
 ┌───────────────────────────────────────┐
 │  [Stats] [Hibernate]                  │  top icon row
 ├───────────────────────────────────────┤
 │                                       │
 │            [pet / egg]                │  pet area (animation)
 │                                       │
 ├───────────────────────────────────────┤
 │  [Feed]  [Heal]  [Play]  [Rest]      │  bottom icon row
 └───────────────────────────────────────┘
```

Use **Up/Down** to switch between icon rows and **Left/Right** to select
an icon. Press **Fire** to open the action menu for the selected icon.

## Stats

Your pet has five stats displayed as percentage bars (lower is better for
you — 0% means the pet is perfectly content):

| Stat          | What happens when it's high              | How to fix        |
| ------------- | ---------------------------------------- | ----------------- |
| **Hunger**    | Pet gets hungry, other stats worsen      | Feed              |
| **Tired**     | Pet is exhausted                         | Put to sleep      |
| **Drained**   | Pet lacks inspiration                    | Relax, mini-games |
| **Sick**      | Pet's health deteriorates                | Give medicine     |
| **Miserable** | Pet is unhappy, everything decays faster | Play              |

Stats interact: if multiple stats are bad, the pet becomes miserable
faster, and miserable makes everything else worse too. Keep on top of
things before they spiral!

Select the **Stats** icon (top-left) and choose "View stats" to see all
five stat bars at once.

## Actions

| Icon     | Options       | What it does                              |
| -------- | ------------- | ----------------------------------------- |
| **Feed** | Feed now      | Reduces hunger (and a bit of drained)     |
| **Heal** | Give medicine | Reduces sick                              |
| **Play** | Play now      | Zeroes miserable (costs some energy)      |
|          | Tic Tac Toe   | Mini-game: draw/win to boost inspiration  |
|          | Lights Out    | Mini-game: solve to boost inspiration     |
|          | Play music    | Play a melody on the buzzer               |
| **Rest** | Sleep         | Pet sleeps until rested (reduces tired)   |
|          | Relax         | Reduces drained (costs some hunger)       |

Each action has a **cooldown** — you'll see "(wait)" next to items that
aren't ready yet. Actions are mutually exclusive: the pet can only do one
thing at a time.

## Mini-Games

Mini-games are found under **Play**. Winning a mini-game rewards your pet
with a burst of inspiration (reduces the drained stat).

### Tic Tac Toe

Classic 3x3 grid. You play as **X** (red) against the computer **O** (black).

- **D-pad**: move cursor
- **Fire**: place your mark
- **Cancel**: quit early

The AI plays optimally, so the best you can do is a draw. Both a win
and a draw award the inspiration bonus.

### Lights Out

A 5x5 grid of lights. Toggling a cell flips it **and** its four neighbours.
Goal: turn all lights off.

- **D-pad**: move cursor
- **Fire**: toggle cell (+ neighbours)
- **Cancel**: quit early

The puzzle is always solvable. Your move count is shown at the bottom.
When you solve it, press **Fire** to collect your inspiration reward.

## Music

The **Play** menu also lets you play melodies through the badge's buzzer:

- Startup jingle
- Never Gonna Give You Up (Rickroll)
- Imperial March
- Sandstorm
- Pink Panther Theme
- Trololo

Playing music does **not** use the play action cooldown.

## Hibernate

If you need to put the badge away for a while, select the **Hibernate**
icon (top row) and choose "Hibernate". This **freezes all stat decay** —
time stands still for your pet until you wake it up.

Use "Wake up" from the same menu to resume.

## Lifecycle

Your pet goes through these phases:

1. **Hatching** — 1-minute countdown, then the pet is born
1. **Active** — stats decay over time, you keep the pet happy with actions
1. **Leaving** — if stats max out for too long, the pet starts leaving
   (countdown depends on how many stats are maxed)
1. **Gone** — the pet has left. Press **Fire** to choose a new pet
   and start a fresh egg

Each new generation inherits the generation counter, so you can see how
many pets you've raised. Past pets are recorded in the **Unicorn Realm**
(see below).

## Traits

Each pet hatches with randomized traits (25%–75% range) that affect
gameplay:

- **Vitality** — determines initial sick level (higher = healthier start)
- **Curiosity** — reduces play action costs (higher = cheaper to play)
- **Resilience** — reserved for future use

## Detailed Stats & Timings

For those who want to min-max their pet care, here are the exact rates.
One game tick = 10 seconds.

### Stat decay rates

| Stat          | Fills in         | What makes it worse                             | What helps                      |
| ------------- | ---------------- | ----------------------------------------------- | ------------------------------- |
| **Hunger**    | ~20 hours        | Time, miserable boost                           | Feed action                     |
| **Tired**     | ~13 hours        | Time, miserable boost                           | Sleep (tiered recovery)         |
| **Drained**   | Interval-based   | Activity, miserable boost                       | Relax action, sleep, mini-games |
| **Sick**      | ~7.6 days (base) | Time + condition decay when other stats are bad | Heal action                     |
| **Miserable** | Interval-based   | Multiple stats above 60%                        | Play action (zeroes it)         |

Stats interact through feedback loops: high miserable boosts
hunger/tired/drained decay rates, bad hunger/tired/drained trigger
accelerated sick decay, and multiple bad stats increase miserable's
growth rate.

### Action durations and cooldowns

| Action    | Duration     | Cooldown | Effect                                        |
| --------- | ------------ | -------- | --------------------------------------------- |
| **Feed**  | 2 ticks      | 12 ticks | Reduces hunger and drained                    |
| **Heal**  | 3 ticks      | 24 ticks | Reduces sick                                  |
| **Relax** | 2 ticks      | 24 ticks | Reduces drained (costs hunger)                |
| **Play**  | 4 ticks      | 48 ticks | Zeroes miserable (costs hunger/tired/drained) |
| **Sleep** | Until rested | —        | Tiered tired recovery, drained recovery       |

Actions are mutually exclusive. During an action and its cooldown, the
corresponding stat's decay is suppressed.

### Leaving thresholds

When stats max out, a leaving countdown starts. The more stats are
maxed, the faster the pet leaves:

| Maxed stats | Time before leaving |
| ----------- | ------------------- |
| 1           | ~20 hours           |
| 2           | ~10 hours           |
| 3           | ~5 hours            |
| 4           | ~2 hours            |

If you reduce the maxed stats back to zero, the countdown resets and the
pet returns to the Active phase.

## Pet Naming

After hatching, you'll be prompted to name your pet using the on-screen
keyboard. A random default name is pre-filled — you can keep it or type
your own (up to 12 characters). The name is shown in the stats view and
saved to flash.

## Unicorn Realm

When a pet leaves, it is recorded in the **Unicorn Realm** — a hall of
fame for your past companions. The last 10 pets are stored, showing their
name, kind, age, and traits.

Access it from the main menu: **Settings > BornPets > Unicorn Realm**.
Use **Up/Down** to scroll through entries, any other button to close.

## Tips

- **Sleep is free** and has no cooldown. Put your pet to sleep whenever
  you're not actively playing — it recovers tired and slows other decay.
- **Hibernate before powering off** to prevent stat decay while the badge
  is stored.
- **Mini-games** are the best way to reduce drained without the hunger
  cost of the relax action.
- **Miserable** is the most dangerous stat — it accelerates everything
  else. Use Play to zero it whenever it builds up.
- Watch the stat bars regularly: catching problems early is much easier
  than recovering from multiple maxed stats.
