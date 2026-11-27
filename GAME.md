# BornPets — How to Play

> **See also:** [README.md](README.md) for project overview, [GAMES.md](GAMES.md) for developer reference on all mini-games, [CLOCK.md](CLOCK.md) for watch/alarm/calendar docs, [CONTACTS_SCREEN.md](CONTACTS_SCREEN.md) for the meshcore chat UI.

The CyberÆgg badge comes with a virtual pet game. Take care of your pet,
keep it happy, and play mini-games to earn rewards.

## Getting Started

When you first navigate to the game screen, you'll see the start screen.
Press **Fire** to begin — you'll be asked to choose your pet:

- **Bartholomeus** — the original CyberÆgg companion (formerly "Snail")
- **Cat** — a feline friend
- **Slug** — a slimy sidekick
- **Panda** — an optional extra pet, installed from a `PETS.CFG` file on
  the badge's USB drive (no reflash — see below)

If money mode is available you'll also pick **With money** / **Without
money** / **Hard mode (US)** here — see [HAX Economy & Hard Mode](#hax-economy--hard-mode).

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
 │            hungry, bored              │  status line (feet of the pet)
 ├───────────────────────────────────────┤
 │   [Feed]  [Heal]  [Play]  [Rest]      │  bottom icon row
 └───────────────────────────────────────┘
```

The pet moves calmly: when nothing is happening it changes to its next idle frame
only once every 5 minutes, which saves battery and spares the e-paper. Actions
(feeding, healing, playing and so on) still animate at full speed, and the screen
redraws at once when you press a button or the pet's state changes.

Use **Up/Down** to switch between icon rows and **Left/Right** to select
an icon. Press **Fire** to open the action menu for the selected icon.

**The status line.** Under the pet, one or two words say what it needs
most, on a white strip so they read over the picture. It is drawn in
black, and in **red** only when the pet is in a health danger zone
(starving, parched, wasted, filthy or poorly). Two words share a line
("hungry, bored") or take two lines when they are too long. The words:

- **needs:** `peckish`, `hungry`, `starving`, `stuffed`, `thirsty`,
  `parched`, `waterlogged`, `sleepy`, `exhausted`, `bored`, `wired`,
  `tipsy`, `sloppy`, `wasted`, `grubby`, `filthy`, `overweight`,
  `underweight`, `unfit`, `hung over`, `sore`, `injured`, `groggy`,
  `poorly`;
- **prompts:** `no insulin` (diabetic and the medication has lapsed) and
  `needs rehab` (dependent and the rehab protection has lapsed);
- **when nothing is wrong:** `asleep`, `well rested`, `content`.

The old corner banners (NEEDS MEDS, NEEDS REHAB, OVERWEIGHT, DRUNK) are
gone: they are now the words `no insulin`, `needs rehab`, `overweight`
and `sloppy`. The badge buzzes only for a danger word or a real alarm
(`hungry`, `thirsty`, `exhausted`, `groggy`, `hung over`, `injured`,
`poorly`), never for `peckish` or `sleepy`.

## Stats

Select the **Stats** icon (top-left) and choose "View stats" to open the
stats view. It has two pages; **Left / Right** flips between them (the
title shows `Needs 1/2` or `Body 2/2`), **Fire** or **Cancel** goes back
to the menu. It always opens on page 1.

| Page | Bars (each one is 0-100%, higher fills the bar)                          |
| ---- | ------------------------------------------------------------------------ |
| **Needs** | Full, Water, Energy, Fun, Clean (100% = spotless)                   |
| **Body**  | Health, Fit, Weight, Booze, Mood, then a line with the pet's conditions |

Every bar shows its comfort band as thin ticks: a value between the ticks
is fine. Full, Water, Fun and Weight have a lower and an upper tick (too
little and too much both hurt); Energy, Clean, Fit and Health have a lower
tick only; Booze has an upper tick only; Mood has none. The Health bar has
one more tick, the **ceiling**: the highest health the pet can recover to,
which rises with fitness and drops for an untreated diabetic. When a value
is outside its band the fill turns **red** (for Mood: below 20%). A black
fill means that bar is fine. The ticks move with the game mode, so Casual
bands are wider than Classic ones.

The line at the bottom of the Body page names the lasting conditions, most
pressing first: injured, sore, hung over, groggy, worn out, diabetic,
dependent, in rehab, well rested. When more than two apply it names two and
counts the rest ("injured, sore +1").

Stats interact: every need that is out of its band drags the mood target
down, and a low mood slows the recovery of health and, when it is very low,
wears health down. Keep on top of things before they spiral!

The Stats menu reads View stats, Hint, Diary, Rolled stats, Friends, Cancel.
"View stats" is a two-page view (page 2 covers the conditions, Fit and
Booze). "Friends" lists the pets your badge has met over the mesh (see
[Friends & the SHDW Channel](#friends--the-shdw-channel) below). Your
lifetime Battle win/loss record shows in the title of the Battle picker
(see [Battles](#battles) below).

"Hint": your pet tells you its most pressing need and what helps (for
example "I'm hungry! / Please feed me."). It never asks for something the
game would refuse: when the action is not possible it says why ("but busy",
"but wait 2h00", "but can't afford"). It also works while hibernating
("Brrr, frozen! / Defrost me!"). Any button returns to the menu.

**Permanent conditions.** Diabetes (from sustained overweight) and
dependence (from sustained heavy drinking) do not wear off on their own.
Rehab clears dependence once it has run its course. No treatment cures
diabetes: Insulin only manages it, and it reverses after the pet has been
lean and fit for a sustained period. Until then, neglect leaves them in place.

"Diary" lists the last 6 things that went wrong (or well), newest first,
three per page (Left / Right flips the page, any other button closes). The
title is "Diary" while everything fits one page, and "Diary 1/2" or
"Diary 2/2" once there are more than three entries.
Each entry shows how long ago it happened ("now", "59m", "23h", "9d") and
the same two lines as the toast. The diary is kept in memory only: a new
pet or a reboot starts it empty.

## Actions

| Icon         | Options          | What it does                                                    |
| ------------ | ---------------- | ---------------------------------------------------------------- |
| **Feed**     | Salad            | Healthiest: raises Full less, barely any Weight, a little Fit and Water |
|              | Apple            | Baseline Full and Weight, a little Water                         |
|              | Frikandel spec   | Deep-fried: raises Full a lot and Weight a real amount, makes you dirtier, mood boost |
|              | Pizza            | Raises Full a lot, big Weight gain; mood boost                   |
|              | Cake             | Barely raises Full, big Mood boost, worst Weight gain. Eat past the Full band and it is a food coma |
| **Heal**     | Give medicine    | Halves an injury, or raises Health when the pet is poorly; on a healthy pet it only numbs (Booze up) |
|              | Insulin          | Only shown once diabetic: lifts the health ceiling cut for a while |
|              | Ozempic          | Lowers Weight (and a little Fit); no treat mood for a while. The strongest cooldown of any action |
|              | Rehab            | Only shown once dependent: a rehab course that clears the dependence |
| **Play**     | Battle           | Fight a friend over the mesh — see [Battles](#battles) below      |
|              | Play now         | Raises Fun and Fit, costs Energy, Full and Water, gets dirtier, shortens groggy |
|              | Tic Tac Toe      | Mini-game: raises Fun more for a better result                   |
|              | Lights Out       | Mini-game: raises Fun more for a better result                   |
|              | Play music       | Play a melody on the buzzer                                      |
| **Rest**     | Sleep            | Pet sleeps until rested (restores Energy)                        |
|              | Relax            | Lowers Fun and strain, raises Energy, costs a little Fit         |
| **Exercise** | Exercise now     | Lowers Weight and Booze, raises Fun; costs Energy, Full and Water, gets dirtier; adds strain |
| **Drink**    | Water            | Raises Water, lowers Booze, eases hangover and groggy; never past 90% |
|              | Cola             | Raises Water, a little Full, Energy and Weight; no Booze         |
|              | Beer             | Raises Water, Booze and Weight a little, lifts Mood              |
|              | Wine             | More Booze than Beer, less Weight                                |
|              | Whiskey          | Most Booze by far, least Weight                                  |

When an action ends badly the pet tells you why. A two-line toast
appears at the top of the pet area: what happened ("food coma", "barely
felt", "groggy", "strained") and the reason under it ("ate too much",
"treat too soon", "overslept", "sport: tired"). A good night says "well
rested". The toasts are silent, and they teach the rules: read them
before you repeat the mistake. They are also logged in the Diary.

A menu row that would end badly shows a small `!` at its right edge. The
row still works: the mark is a warning, not a lock. Move the cursor onto
it and the title bar reads "risk: " plus what would happen ("risk: food
coma"), so you can think again before you press. For a dose
(food, drink, medicine, play, exercise, relax, wash, wake up) the marker is
exactly what the action would do. For Sleep it is the engine's forecast of a
night slept until rested. A row
you cannot use shows no mark; selecting it says why it is refused. On the
Feed page the title shows the selected food's full name, so the Frikandel
row ("Frikandel spec") reads "Frikandel speciaal" there. In Hard mode that
row and its title both say "Hamburger".

A row that is not ready yet shows the time left instead of its price, e.g.
"Frikandel spec 50s", "Aspirine 4h00" or "Ozempic 47h59". Actions are mutually exclusive: the pet can only do one thing at
a time.

## HAX Economy & Hard Mode

At pet select you can choose to play **with money** — an optional layer
that gives your pet a **HAX** balance (a fresh egg starts at 100).

**Earning HAX**

- Winning a mini-game
- Winning a mesh Battle
- **OnlyPets** — a work/hobby action under **Play**. It earns HAX; if the
  pet is broke, a forced-work branch kicks in and pays more.

**Spending HAX** — when money mode is on, food, drinks, Aspirine,
Insulin, Ozempic and Rehab all cost HAX. Priced items show their cost
(e.g. `Salad 15H`); anything you can't afford is disabled until you earn
more.

**Hard mode (US)** is a third choice at pet select. Prices flip so that
staying healthy is expensive — healthy food and drinks cost more and
treatments are pricier. In this mode the *Frikandel speciaal* is even
relabelled a **Hamburger**.

Play **without money** for the classic experience — no prices, no HAX.

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
Frikandel speciaal and Pizza add real weight, and Cake is the worst offender by far
(great mood boost, terrible for the waistline). Use the **Exercise** icon
(top row) whenever you notice the Fit bar dropping to bring weight back
down.

If weight stays overweight (Fit below ~40%) for a sustained period —
several days of neglecting exercise — your pet develops **type 2
diabetes**. The moment it happens, a buzzer sounds and the badge shows
a full-screen "TYPE 2 DIABETES — Give medication soon" alert for at
least 3 seconds before returning to normal. It does not wear off: only a
long lean, fit stretch reverses it (see "Permanent conditions"), and
until then it needs management. Once diabetic, **Insulin** appears as a new
option under the **Heal** icon. Without a dose the pet's health ceiling
drops and the status line says "no insulin"; a dose protects the pet for a
while before it needs another.

## Drinks & Alcoholism

Same shape as the weight/diabetes arc, on a separate track. Select the
**Drink** icon (top row) to choose Water, Cola, Beer, Wine, or Whiskey.
Water and Cola never affect drunk; Beer/Wine/Whiskey do, with Whiskey
hitting hardest. Unlike weight, drunk sobers up on its own over several
hours — no action needed — so staying alcoholic-track-worthy requires
repeated drinking, not just one binge.

If your pet stays drunk (past a threshold) for a sustained period —
several days of repeated heavy drinking — it develops **dependence**. It
does not wear off on its own; a full course of **Rehab**, an option under
the **Heal** icon (alongside Give medicine, Insulin, and Ozempic), clears
it. A dependent pet clears alcohol more slowly and, when it is nearly sober
and awake, craves a drink: its mood drops until it drinks or has been
through rehab.

**Ozempic**, also under Heal, is a separate, stronger weight-loss
treatment — usable any time, not just once diabetic — but on a much
longer cooldown than Exercise, so it's a once-in-a-while boost rather
than a routine option.

Whenever medication has lapsed, the status line under the pet says
**"no insulin"** until you re-dose — the pet itself keeps showing its
normal animation above it. A dependent pet whose rehab has lapsed shows
**"needs rehab"** the same way.

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
the same lasting conditions that hurt everyday care also make your
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
never affects its health, mood, or anything else about the pet's
actual wellbeing; the worst that happens is you lose the bragging
rights. Wins and losses are tracked two ways: a simple lifetime counter
on your pet overall (visible in the title of the Battle picker, as
"Battle 3W-1L"), and a **head-to-head
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

> **Note:** this section describes the old v1 model (Hunger, Tired, Drained,
> Sick, Miserable, interval-based decay and per-action cooldowns). It is
> superseded by the v3 sections above (Stats, Actions, the status line) and
> by the player's guide. It is kept for reference only.

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

Opens a **Debug** menu with, in order:

- **Engine info** — a toast with the status word (`!` in danger, `zz`
  asleep, `nt` night), mood / mood target, health and the time to the
  engine's next wake ("23m", "2h05"): how a tester checks the wake
  scheduler
- **Skip 10 min** / **Skip 1 hour** / **Skip 1 day** — fast-forward the
  engine through the normal update path, like real time
- **Comfort** — every need mid-band, mood and health 70 %
- **Hungry**, **Thirsty**, **Stuffed**, **Tired**, **Bored**, **Dirty**,
  **Fat**, **Thin**, **Unfit**, **Drunk** — put that need just past its
  line so the status word names it
- **Hung over**, **Groggy**, **Injured** — start that timer at its dose
  value
- **Diabetic**, **Dependent** — set the condition directly
- **Clear conditions** — clears injury, hangover, grogginess, dependence,
  diabetes, rehab and reversal
- **Clear cooldowns** — clears the medicine, Ozempic, insulin, mini-game
  and battle cooldowns
- **Reset battle record** — zeroes your own wins/losses and every
  known friend's cached head-to-head record, useful for verifying
  battle-sync fixes without waiting for old data to age out

A poke on a pet that is hatching, gone or hibernating shows its reason.

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
- **Scrolling menus wrap around**: pressing Up at the top of any list
  (the action menu, Debug menu, Friends list, Battle target picker,
  Unicorn Realm) jumps to the bottom instead of doing nothing, and Down
  at the bottom wraps back to the top — the fastest way to reach the
  last item is often Up once from the first row.
