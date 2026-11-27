#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/hackers.typ": *
#chapter("ch-secrets")[Secrets]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

#grid(columns: (1fr, auto), column-gutter: 5mm, align: horizon,
  [
    There is a page in the badge that no icon opens. It was built to test
    the pet without waiting for days. It lets you set a need, start a
    condition, or skip the clock. It is not part of the game, and the pet
    does not forgive it. You may peek. This chapter is the map.
  ],
  stamp("TOP SECRET"))

== The code

Stand on the Main game screen with nothing open: no menu, no mini-game. Then
press the nine buttons below, in this order.

#secret-code()

When the last press lands, the #w("Debug") menu opens. That final Fire is
used up by the code, so it does not also open the icon under the cursor.

A wrong press starts the count again. If the wrong press is an Up, it counts
as the first step, so you can try again at once.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    The menu has one row for each trick, then #w("Cancel"). Up and Down
    move. Left and Right turn the page. Fire does the row.
  ],
  screen("debug-menu", width: shot-m, caption: [The Debug menu.]))

#warn[A cheat is a real change. The badge saves it, like anything else the
pet goes through. There is no undo, apart from the clear rows below.]

== Every row

*Look.* #w("Engine info") shows a toast of what the engine thinks, then
leaves the menu. Nothing changes. The next section reads it.

*Skip the clock.* These run the pet through the real rules, as if the time
had passed, so the pet gets hungry, tired and old on the way. They work on
any pet, even a frozen one. They do not touch the badge's clock.

#word-table(
  table.header([*Row*], [*What it does*]),
  w("Skip 10 min"), [Ten minutes pass at once.],
  w("Skip 1 hour"), [One hour passes at once.],
  w("Skip 1 day"), [One day passes at once.],
)

*Poke a need.* Each row writes the pet's level, so the next update sees it
as if the pet had got there alone. The level lands clearly past the line
that names it, so the status word shows.

#word-table(
  table.header([*Row*], [*What it does*]),
  w("Comfort"), [Fullness, hydration, fun and weight go to the middle of
    their bands. Energy and fitness go halfway between their low edge and
    full. Dirt and strain go to half their top line. Booze goes to zero. Mood
    and health go to #v("COMFORT_PCT").],
  w("Hungry"), [Fullness drops under the danger line.],
  w("Thirsty"), [Hydration drops under the danger line.],
  w("Stuffed"), [Fullness goes past the top of its band.],
  w("Tired"), [Energy drops under the line where damage starts.],
  w("Bored"), [Fun drops under the bottom of its band.],
  w("Dirty"), [Dirt goes just over the danger line.],
  w("Fat"), [Weight goes past the top of its band.],
  w("Thin"), [Weight drops under the bottom of its band.],
  w("Unfit"), [Fitness drops under the bottom of its band.],
  w("Drunk"), [Booze goes just over the danger line.],
)

*Start a condition.* These set the clock of a condition to its full length
(@ch-conditions).

#word-table(
  table.header([*Row*], [*What it does*]),
  w("Hung over"), [A hangover for #v("HANGOVER_MIN").],
  w("Groggy"), [A groggy spell for #v("GROGGY_MIN").],
  w("Injured"), [An injury for #v("DOSE_INJURY_MIN").],
  w("Diabetic"), [The pet is diabetic. It needs its insulin now.],
  w("Dependent"), [The pet is dependent. It needs rehab.],
)

*Tidy up.*

#word-table(
  table.header([*Row*], [*What it does*]),
  w("Clear conditions"), [Ends every condition at once: injury, hangover,
    groggy, dependence, diabetes, rehab and the rest.],
  w("Clear cooldowns"), [Frees the medicine, Ozempic and insulin waits, the
    four mini-games and Battle. It also ends a working insulin dose, so a
    diabetic pet's ceiling penalty returns.],
  w("Reset battle record"), [Sets this pet's wins and losses to zero, and
    every friend's record too (@ch-friends).],
)

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *When a row says no.* A pet that is still an egg, already gone or
    frozen takes no poke. The usual reason shows, the same as for any
    refused action (@ch-care), and the menu stays open. The skip rows
    and the look row still work.

    *When it works.* A skip, a poke or the reset shows #w("cheat applied"),
    and the menu closes.
  ],
  hero-pic("critical"))

== Engine info, token by token

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  screen("engine-info", width: shot-l, caption: [A read-out.]),
  [
    The toast has a few short lines. Here it says #w("Content"), then
    #w("m<mood>/<target>"), then #w("h<health>") and #w("w<wait>"). The
    numbers are percent, and they are read live.

    - *The word.* One of the words in the table below. It is the single
      biggest problem. #w("Content") means no need is large enough to name.
    - *The flags.* A second line shows up only when a flag is on. #w("!") is
      a danger word on the status line. #w("zz") is asleep. #w("nt") is a
      night's sleep, not a nap (@ch-care). More than one may show, side by
      side.
    - *Mood.* #w("m<mood>/<target>") is the mood now, a slash, and the
      target it is walking to (@ch-mood).
    - *Health.* #w("h<health>") is the health now.
    - *The wait.* #w("w<wait>") is the time until the engine next needs to
      wake, like `w55m` or `w2h05`. Seconds show as a plain count,
      minutes as an m, hours as an h. When the pet sleeps until something
      outside changes (frozen or gone), the wait is #w("w-").
  ])

The word is a coarse cousin of the status line (@ch-reading). The line has
finer words, such as peckish, hungry and starving. Engine info folds each
group into one.

#word-table(see: true,
  table.header([*Word*], [*What it says*], [*Read on*]),
  w("Content"), [Nothing is large enough to name.], [@ch-mood],
  w("Hungry"), [Fullness is low.], [@ch-needs],
  w("Stuffed"), [Fullness is high.], [@ch-needs],
  w("Thirsty"), [Hydration is low.], [@ch-needs],
  w("Waterlogged"), [Hydration is too high.], [@ch-needs],
  w("Tired"), [Energy is low.], [@ch-needs],
  w("Bored"), [Fun is low.], [@ch-needs],
  w("Wired"), [Fun is too high.], [@ch-needs],
  w("Sloppy"), [Booze is above its band.], [@ch-needs],
  w("Grubby"), [Dirt is above its band.], [@ch-needs],
  w("Overweight"), [Weight is too high.], [@ch-needs],
  w("Underweight"), [Weight is too low.], [@ch-needs],
  w("Unfit"), [Fitness is too low.], [@ch-needs],
  w("Hung over"), [A hangover is running.], [@ch-conditions],
  w("Injured"), [An injury is running.], [@ch-conditions],
  w("Groggy"), [A groggy spell is running.], [@ch-conditions],
  w("Poorly"), [Health is low.], [@ch-mood],
)

#joke[A cheat is a shortcut through the hedge. It gets you there fast, and
the pet will not thank you for it.]
