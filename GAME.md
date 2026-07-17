# BornPets — How to Play

> **See also:** [README.md](README.md) for project overview, [GAMES.md](GAMES.md) for developer reference on all mini-games, [CLOCK.md](CLOCK.md) for watch/alarm/calendar docs, [CONTACTS_SCREEN.md](CONTACTS_SCREEN.md) for the meshcore chat UI.

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
 │  [Stats] [Hibernate] [Exercise] [Drink] │  top icon row
 ├───────────────────────────────────────┤
 │                                       │
 │            [pet / egg]                │  pet area (animation)
 │                                       │
 ├───────────────────────────────────────┤
 │   [Feed]  [Heal]  [Play]  [Rest]      │  bottom icon row
 └───────────────────────────────────────┘
```

Use **Up/Down** to switch between icon rows and **Left/Right** to select
an icon. Press **Fire** to open the action menu for the selected icon.

## Stats

Your pet has six stats displayed as percentage bars (lower is better for
you — 0% means the pet is perfectly content):

| Stat          | What happens when it's high                                      | How to fix        |
| ------------- | ---------------------------------------------------------------- | ----------------- |
| **Hunger**    | Pet gets hungry, other stats worsen                              | Feed              |
| **Tired**     | Pet is exhausted                                                 | Put to sleep      |
| **Drained**   | Pet lacks inspiration                                            | Relax, mini-games |
| **Sick**      | Pet's health deteriorates                                        | Give medicine     |
| **Miserable** | Pet is unhappy; speeds up drained decay and sick-condition decay | Play              |
| **Weight**    | Pet gets overweight; sustained overweight leads to diabetes      | Exercise          |

Stats interact: if multiple stats are bad, the pet becomes miserable
faster, and miserable makes everything else worse too. Keep on top of
things before they spiral!

Select the **Stats** icon (top-left) and choose "View stats" to see all
six stat bars at once (labeled "Fit" for weight — 100% = lean, 0% =
obese), "Health status" for a plain-language readout of Diabetic /
Overweight / Alcoholic / Fit% (plus your lifetime Battle win/loss
record — see [Battles](#battles) below), with a short explanation of
what triggers each, or "Friends" for the pets your badge has met over
the mesh (see
[Friends & the SHDW Channel](#friends--the-shdw-channel) below).

## Actions

| Icon         | Options          | What it does                                                    |
| ------------ | ---------------- | ---------------------------------------------------------------- |
| **Feed**     | Salad            | Healthiest — less filling, barely any weight gain               |
|              | Apple             | Baseline hunger relief and weight gain                          |
|              | Burger           | Filling, but a real weight hit                                  |
|              | Pizza            | Very filling — but the biggest weight gain short of dessert      |
|              | Cake             | Barely touches hunger, big mood boost, worst weight gain by far |
| **Heal**     | Give medicine    | Reduces sick                                                     |
|              | Insulin          | Only shown once diabetic — suppresses the diabetes sick-penalty for a while |
|              | Ozempic          | Accelerated weight loss — not gated on being diabetic; appetite-suppressing (also relieves a little hunger), but the strongest cooldown of any action |
|              | Rehab            | Only shown once alcoholic — suppresses the alcoholism sick-penalty for a while |
| **Play**     | Battle           | Fight a friend over the mesh — see [Battles](#battles) below      |
|              | Play now         | Zeroes miserable (costs some energy)                             |
|              | Tic Tac Toe      | Mini-game: draw/win to boost inspiration                         |
|              | Lights Out       | Mini-game: solve to boost inspiration                            |
|              | Play music       | Play a melody on the buzzer                                      |
| **Rest**     | Sleep            | Pet sleeps until rested (reduces tired)                          |
|              | Relax            | Reduces drained (costs some hunger)                              |
| **Exercise** | Exercise now     | Reduces weight (costs some hunger and tired, small drained bonus) |
| **Drink**    | Water            | No effect on drunk, refreshing (drained relief)                  |
|              | Cola             | No effect on drunk; a little weight gain, good drained relief    |
|              | Beer             | Baseline drunk gain and weight gain                              |
|              | Wine             | More drunk than Beer, less weight gain                           |
|              | Whiskey          | Most drunk by far, least weight gain, strongest drained relief   |

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

## Weight & Diabetes

Weight is a slow, multi-day stat — it isn't something you need to manage
hour-to-hour like hunger. It drifts up gradually over time, and *what*
you feed matters a lot: Salad barely moves it, Apple is the baseline,
Burger and Pizza add real weight, and Cake is the worst offender by far
(great mood boost, terrible for the waistline). Use the **Exercise** icon
(top row) whenever you notice the Fit bar dropping to bring weight back
down.

If weight stays overweight (Fit below ~40%) for a sustained period —
several days of neglecting exercise — your pet develops **type 2
diabetes**. The moment it happens, a buzzer sounds and the badge shows
a full-screen "TYPE 2 DIABETES — Give medication soon" alert for at
least 3 seconds before returning to normal. This is permanent: there's
no cure, only management. Once diabetic, **Insulin** appears as a new
option under the **Heal** icon. Skipping it for too long makes sick
decay faster; a dose protects the pet for a while before it needs
another.

## Drinks & Alcoholism

Same shape as the weight/diabetes arc, on a separate track. Select the
**Drink** icon (top row) to choose Water, Cola, Beer, Wine, or Whiskey.
Water and Cola never affect drunk; Beer/Wine/Whiskey do, with Whiskey
hitting hardest. Unlike weight, drunk sobers up on its own over several
hours — no action needed — so staying alcoholic-track-worthy requires
repeated drinking, not just one binge.

If your pet stays drunk (past a threshold) for a sustained period —
several days of repeated heavy drinking — it develops permanent
**alcoholism**, exactly like diabetes: no cure, only management via
**Rehab**, a new option under the **Heal** icon (alongside Give
medicine, Insulin, and Ozempic). Skipping it makes sick decay faster,
same as skipping Insulin; a session protects the pet for a while.

**Ozempic**, also under Heal, is a separate, stronger weight-loss
treatment — usable any time, not just once diabetic — but on a much
longer cooldown than Exercise, so it's a once-in-a-while boost rather
than a routine option.

Whenever medication has lapsed, a persistent **"NEEDS MEDS"** banner
shows in the corner of the pet screen until you re-dose — the pet itself
keeps showing its normal animation underneath, the banner doesn't
replace it.

## Friends & the SHDW Channel

Every badge running this firmware auto-joins a private mesh channel
called **SHDW** — no setup needed. While your pet is active, the badge
quietly broadcasts a small presence beacon on it every 15 minutes. When
another SHDW badge is in LoRa range and its beacon comes in, your pet
"meets" theirs:

- **Meeting a badge for the first time** — a new friend, a bigger
  happiness boost (drops **Miserable** by a lot)
- **Meeting a known friend again**, after a few hours apart — "spending
  time together", a smaller happiness boost
- **Meeting a known friend again too soon** — no boost (they need some
  time apart before it counts as quality time again)

Check who your pet has met via **Stats > Friends** — a scrollable,
clickable menu of every friend seen so far, ordered with whoever you've
**seen most recently at the top** (a friend jumps back to the top of
the list the moment a fresh beacon from them comes in). Use **Up/Down**
to move the cursor and **Fire** on a friend to see their details: kind,
how long you've known them, how long since their last beacon ("Last
seen"), their cached combat stats, and your head-to-head Battle record
against them specifically (see [Battles](#battles) below). Any button
on the detail screen goes back to the list; any button on the list
itself (other than Up/Down/Fire) closes the whole screen. Long names
wrap onto a second line instead of running off the
edge of the screen.

This only works between two physical badges, both running this firmware
and both in LoRa range of each other and on the SHDW channel — there's
no simulated version of meeting a friend.

## Battles

Every pet has four combat stats — **Attack**, **Defense**, **Speed**,
and **HP** — derived automatically from its regular stats, so there's
nothing extra to manage:

| Combat stat | Driven mainly by                              |
| ----------- | ---------------------------------------------- |
| Attack      | Curiosity trait + how well cared-for the pet is |
| Defense     | Resilience trait + how lean/fit the pet is      |
| Speed       | How lean/fit the pet is + Vitality trait        |
| HP          | Vitality trait, reduced if the pet is sick      |

Being diabetic or alcoholic takes a flat toll on Attack/Defense/Speed —
the same permanent conditions that hurt everyday care also make your
pet a weaker fighter.

Select **Battle** — now the first option under the **Play** icon — to
challenge a friend from your Friends list. Battling is **instant**:
your badge resolves the whole fight immediately using your pet's live
stats and your friend's most recently broadcast combat stats, no
waiting around. You'll see a result screen with both pets' remaining
HP and a WIN/LOSE banner right away.

Your friend's badge finds out asynchronously — whenever it next
receives your battle broadcast on SHDW, it registers the result (from
their side, a loss for them if you won) and shows its own toast, even
if they weren't looking at the Battle screen.

**Battle HP is entirely separate from your pet's real health** — it's
computed fresh for each fight and discarded afterward. Losing a battle
never affects `sick`, `miserable`, or anything else about the pet's
actual wellbeing; the worst that happens is you lose the bragging
rights. Wins and losses are tracked two ways: a simple lifetime counter
on your pet overall (visible in Health Status), and a **head-to-head
record against each specific friend** (visible on that friend's detail
screen under Stats > Friends). Both badges update the same
head-to-head tally for every battle between them — win it as the
challenger and your friend's copy shows the matching loss, and vice
versa — so the record reads the same from either side. Battling has
its own cooldown (120 seconds), same as every other action.

Like Friends, actually having your opponent's badge learn the outcome
needs two physical badges in range of each other on SHDW — but you can
always battle solo against any cached friend regardless of whether
they're currently nearby.

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
| **Hunger**    | ~20 hours        | Time only                                       | Feed action                     |
| **Tired**     | ~13 hours        | Time only                                       | Sleep (tiered recovery)         |
| **Drained**   | Interval-based   | Activity; interval shortens 90→30 ticks when miserable ≥ 80 % | Relax action, sleep, mini-games |
| **Sick**      | ~7.6 days (base) | Time + condition decay when other stats are bad; extra penalty while diabetic and unmedicated | Heal / medicate action |
| **Miserable** | Interval-based   | Multiple stats above 60%                        | Play action (zeroes it)         |
| **Weight**    | Very slow (days) | Time (tiny passive rate) + a little extra on each Feed | Exercise action           |

Stats interact through feedback loops: a bad miserable accelerates the
drained interval and the sick condition rate, bad hunger/tired/drained
trigger sick condition decay, and multiple bad stats increase
miserable's growth rate.

### Happiness floor when stats are bad

The internal `miserable` value (which the displayed Happy bar inverts)
has a hard floor whenever the pet is in trouble:

| Condition                         | Floor on `miserable` (= cap on Happy) |
| --------------------------------- | ------------------------------------- |
| Pet is in the **Leaving** phase   | 50 % (Happy ≤ 50 %)                   |
| Each primary stat above critical  | +20 % per stat (Happy ≤ 80 / 60 / 40 / 20 %) |

The two rules evaluate independently and the **higher** floor wins, so
a leaving pet with 4 critical stats sits at the severe floor of 80 %
miserable rather than the leaving floor of 50 %.

`Play` still resets miserable, but only down to whichever floor is
currently active — so playing with a critically distressed pet brings
it to "as happy as it can be right now", not all the way to 100 %
happy.  Once stats clear and the pet is back in the Active phase, the
floor drops to 0 again and Play's reset works as before.

### Action durations and cooldowns

| Action    | Duration     | Cooldown | Effect                                                                 |
| --------- | ------------ | -------- | ---------------------------------------------------------------------- |
| **Feed**  | 2 ticks      | 12 ticks | Reduces hunger and drained                                             |
| **Heal**  | 3 ticks      | 24 ticks | Reduces sick                                                           |
| **Relax** | 2 ticks      | 24 ticks | Reduces drained (costs hunger)                                         |
| **Play**  | 4 ticks      | 48 ticks | Resets miserable down to the active floor (costs hunger/tired/drained) |
| **Sleep** | Until rested | —        | Tiered tired recovery, drained recovery                                |
| **Exercise** | 3 ticks   | 36 ticks | Reduces weight (costs hunger/tired, small drained relief)              |
| **Medicate** | 2 ticks   | 2-3 hours | Diabetic only — suppresses the diabetes sick-penalty until it lapses  |

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

## Debug Cheats

For testing the weight/diabetes arc without waiting several real days,
there's a hidden cheat menu: from the main game screen (no modal or
mini-game open), press **Up, Up, Down, Down, Left, Right, Left, Right,
Fire** — a Konami-code-style sequence adapted to this badge's button
set. A mistimed press just resets the tracker; arrow presses still move
the nav cursor normally while you're attempting it.

Opens a **Debug** menu with:

- **Force overweight** — pushes weight just over the 60% trigger
- **Trigger diabetes** — flips the pet diabetic immediately, skipping
  the multi-day onset timer
- **Clear diabetes** — resets diabetic status and overweight progress,
  so the arc can be re-tested without starting a new pet
- **Force drunk** — pushes drunk just over its trigger
- **Trigger alcoholism** — flips the pet alcoholic immediately, skipping
  the multi-day onset timer
- **Clear alcoholism** — resets alcoholic status and drunk progress
- **Reset battle record** — zeroes your own wins/losses and every
  known friend's cached head-to-head record, useful for verifying
  battle-sync fixes without waiting for old data to age out
- **Skip 1 hour** / **Skip 1 day** — fast-forwards the engine, useful
  for any time-based mechanic, not just this one

This is a developer tool, not part of normal play — no cooldowns, no
gating beyond having an active pet.

## Forcing a screen de-ghost

E-paper builds up visible ghosting over many fast partial updates. The
badge already promotes to a full refresh automatically once enough
pixels have changed, but there's also a hidden combo to force a clean
de-ghost immediately, on **any** screen — not just the pet screen:
press **Down, Down, Up, Up, Right, Left, Right, Left, Fire** (the debug
cheat combo above, run in reverse). The panel goes solid black, then
solid white, then redraws whatever screen was showing. This is a
display utility, not a pet mechanic — it works with no pet active and
regardless of which screen you're on.

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
