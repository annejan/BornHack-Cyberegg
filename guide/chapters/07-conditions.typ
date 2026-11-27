#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/mood.typ": *
#chapter("ch-conditions")[Conditions]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

A need is a number that moves. A *condition* is a state that sticks. It has a
cause, it lasts for a while, and it goes away only in a certain way. This
chapter has one section for each, in the same order: how the pet gets it,
what it does, and how to get rid of it. The stats view (@ch-needs) names the
lasting ones under its bars: #w("injured"), #w("sore"), #w("hung over"),
#w("groggy"), #w("worn out"), #w("diabetic"), #w("dependent"), #w("in rehab")
and #w("well rested"). With more than two, a count shows after them, like
#w("+<n>").

#pic-row((("sick", [Hung over.]), ("tired", [Worn out.]), ("healing", [Mending.])))

== Hung over

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *How it starts.* A pet that drinks past #v("BOOZE_HI_PCT") booze is
    drunk. When the booze then falls under #v("BOOZE_CLEAR_PCT"), the party
    is over and the pet is #w("hung over") for #v("HANGOVER_MIN"). The clock
    restarts each time a binge ends. It does not stack.

    *What it does.* The mood target loses #v("MOOD_PEN_HANGOVER_PCT").
    Energy drains at #v("ENERGY_MULT_HANGOVER_PCT") of the usual speed while the
    pet is awake. As the first word it buzzes (@ch-reading).
  ],
  screen("hung-over", width: shot-m, caption: [Hung over.]))

*How to get rid of it.*
- *Water.* Each glass takes #v("DOSE_WATER_HANGOVER_MIN") off the clock.
- *Sleep.* An asleep pet's hangover runs #v("HANGOVER_ASLEEP_MULT") times as
  fast.
- *Exercise.* A session takes #v("DOSE_EXERCISE_HANGOVER_MIN") off.
- *Time.* Nothing else works. Aspirine does not touch it.

Wake a pet that is still hung over and the toast says #w("hung over") with
#w("needs water"). The hint is right: the pet needs water.

#tip[Booze clears slower when the pet is thirsty and slower again when it is
#w("dependent"). Water first, always.]

== Groggy

*How it starts.* A pet that sleeps with full energy is oversleeping. The
badge counts those minutes. When the pet wakes with more than
#v("GROGGY_LINE_MIN") of them, it is #w("groggy") for #v("GROGGY_MIN"). It
also loses #v("GROGGY_ENERGY_PCT") energy and #v("GROGGY_FIT_PCT") fitness.
The toast is #w("groggy") with #w("overslept"). A pet left alone wakes by
itself after #v("OVERSLEEP_WAKE_MIN") of oversleeping, which is past that
line, so it wakes groggy. (A rest at a magic place is the exception: it wipes the count, see @ch-friends.) It also wakes after #v("SELF_WAKE_MIN") of sleep in
all. A worn-out pet is full sooner, so it oversleeps sooner. Being awake with energy under #v("OVERSLEEP_RESET_PCT")
wipes the count, and so does going to bed under that line. A rested pet that
goes back to bed keeps its count.

*What it does.* The mood target loses #v("MOOD_PEN_GROGGY_PCT"). As the first word it buzzes.

*How to get rid of it.* Water takes #v("DOSE_WATER_GROGGY_MIN") off, play
#v("DOSE_PLAY_GROGGY_MIN"). A session of exercise ends it on the spot, and so
does a coffee, which also gives the lost energy back. The
best cure is to wake the pet before it oversleeps. The #w("Sleep") row in the
#w("Rest") menu warns when the sleep ahead would end that way.

*Coffee, and too much of it.* A coffee ends the spell on the spot and gives
back the #v("GROGGY_ENERGY_PCT") energy it took: the toast is
#w("perked up") with #w("had a coffee"). Every cup also adds caffeine that
takes #v("COFFEE_WINDOW_MIN") to wear off. Cup number #v("COFFEE_WIRED_CUPS")
inside that time wires the pet: its fun goes over #v("FUN_HI_PCT"), enough to
stay #w("wired") for #v("COFFEE_WIRED_MIN"), with the toast #w("wired") and
#w("too much coffee"). An older cup still counts for what is left of it, so a
steady drip of coffee gets there too.

*Grumpy is not groggy.* Wake a pet by hand while its energy is under
#v("GRUMPY_ENERGY_PCT") and it wakes #w("grumpy"), with #w("woken too early").
That costs #v("GRUMPY_PCT") of mood, once. It is a toast, not a condition.

== Worn out

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *How it starts.* A *night* is a sleep that starts with energy under
    #v("DOSE_NIGHT_ENERGY_PCT"), or a collapse. A night shorter than
    #v("NIGHT_SHORT_MIN") is a *short night*. After #v("DEPRIVED_NIGHTS")
    short nights in a row the pet is #w("worn out"). The toast says
    #w("worn out") with #w("short nights"). Naps never count. A night of
    #v("NIGHT_SHORT_MIN") or more ends the run.

    *What it does.* While asleep the pet only refills to
    #v("ENERGY_CAP_DEPRIVED_PCT") energy. The mood target loses
    #v("MOOD_PEN_DEPRIVED_PCT"). Health heals at #v("REC_DEPRIVED_PCT") of its
    speed. Exercise strains #v("DOSE_STRAIN_DEPRIVED_PCT") as hard.
  ],
  sick-sleep-pics())

*How to get rid of it.* Sleep long. A night of at least #v("NIGHT_GOOD_MIN")
is a *good night*. After #v("CLEARING_NIGHTS") good nights since the last short
one, the pet is clear. A night in between, of #v("NIGHT_SHORT_MIN") to just
under #v("NIGHT_GOOD_MIN"), stops the short run but does not count as good.
Put the pet to bed fed, watered and sober, and do not wake it early.

#joke[#v("DEPRIVED_NIGHTS") short nights and the pet is worn out. Three nights is also how
long a badge hacker lasts at a hacker camp. Nobody has a cure for that.]

== Sore and injured

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *How it starts.* Exercise adds strain (@ch-needs). Strain over
    #v("SORE_PCT") makes the pet #w("sore"). A session that ends with strain
    over #v("STRAIN_INJ_PCT") tears a muscle: the pet is #w("injured") for
    #v("DOSE_INJURY_MIN"), loses #v("DOSE_INJURY_HEALTH_PCT") health and gets no
    fitness from that session. In Casual the line is
    #v("STRAIN_INJ_PCT", preset: casual).

    *What it does.* #w("sore") is only a warning. #w("injured") pulls the
    mood target down by #v("MOOD_PEN_INJURED_PCT") and, as the first word, buzzes. An
    injured pet refuses #w("Play now"), #w("Exercise now") and
    #w("Only pets") with #w("injured").
  ],
  screen("injured", width: shot-m, caption: [Injured.]))

*How to get rid of it.* Sore goes away alone: strain clears a whole bar in
#sweep("STRAIN_CLEAR_PM"), and #w("Relax") takes #dose-pct("Relax", "strain")
off at once. An injury runs its clock. #w("Aspirine") from #w("Heal") takes
#v("DOSE_HEAL_INJURY_MIN") off it, but adds #v("DOSE_MED_BOOZE_PCT") booze, and
the row is shut for #v("DOSE_MED_CD_MIN") afterwards.

The toast says #w("torn muscle") (or #w("strained") when a session went
wrong but nothing tore) and names the cause in a second line, one of
#w("sport: hungry"), #w("sport: thirsty"), #w("sport: tired"),
#w("sport: heavy"), #w("sport: worn out") or #w("sport: too much"). The list
is in that order: the first handicap that fits is blamed.

== Dependence and rehab

*How it starts.* The badge keeps a running total of the time the booze bar
spends over #v("BOOZE_HI_PCT"). Time under #v("BOOZE_CLEAR_PCT") pays the
total back, at a quarter of the speed. In between, the total stands still.

#dependence-bar()

At #v("DEPENDENCE_ONSET_MIN") on the total, the pet is #w("dependent"). A pet
that has just left rehab is more fragile: for #v("RECOVERING_MIN") after rehab
the line is only #v("DEPENDENCE_RELAPSE_MIN"). The *dependence alarm* sounds
at the onset (see below).

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *What it does.* Each drink gives less mood and energy. Booze clears at
    #v("BOOZE_CLEAR_SLOW_PCT") of the speed. And there are *cravings*. An
    awake, dependent pet with booze under #v("MOOD_CRAVING_BOOZE_PCT")
    loses #v("MOOD_PEN_CRAVING_PCT") of target. The status line shows the
    prompt #w("needs rehab").
  ],
  screen("needs-rehab", width: shot-m, caption: [Needs rehab.]))

*How to get rid of it.* Open the #w("Rehab") row. It costs #price("Rehab")
and runs for #v("DOSE_REHAB_MIN"). The row says #w("no need") if the pet is
not dependent. While a rehab runs it says #w("wait <time>"), the time left. While it runs the pet is #w("in rehab"):
the mood loses #v("MOOD_PEN_REHAB_PCT"), and sleep goes badly. Sleeping with
a rehab running gives the reason #w("withdrawal"). When it ends the pet is
free: no #w("dependent"), a clean total.

*Relapse.* A drink during rehab ends the rehab at once. The pet loses
#v("DOSE_RELAPSE_MOOD_PCT") of mood, and the toast says #w("relapse!") with
#w("rehab lost"). The pet is still dependent.

#warn[Rehab is dear in #w("Hard mode (US)"). See @ch-hard. In every mode, the
cheap cure is to never get there.]

== Diabetes

*How it starts.* It works like dependence, with weight. The badge keeps a
running total of the time the weight spends over
#v("DIABETES_WEIGHT_HI_PCT"). Time under #v("DIABETES_WEIGHT_LO_PCT") pays it
back, at a quarter of the speed. In between, it stands still.

#diabetes-bar()

At #v("DIABETES_ONSET_MIN") on the total, the pet is #w("diabetic"). The
*diabetes alarm* sounds at the onset (see below).

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *What it does.* The health ceiling loses #v("CEILING_DIABETIC_PCT")
    points, unless insulin is working. The status line shows #w("no insulin")
    until you give some.

    *Insulin.* The #w("Insulin") row costs #price("Medicate"). One dose works
    for #v("DOSE_INSULIN_MIN"). The row opens again in the last
    #v("DOSE_INSULIN_WINDOW_MIN") of a dose, so a dose is never wasted. Before
    that the row says #w("wait <time>"), the time left. A pet that is not
    diabetic gets #w("no need").
  ],
  screen("no-insulin", width: shot-m, caption: [No insulin.]))

*Reversal.* Diabetes can be undone. Time that the weight is under
#v("DIABETES_WEIGHT_LO_PCT") *and* the fitness is over #v("REVERSAL_FIT_PCT")
counts toward a cure. At #v("DIABETES_REVERSAL_MIN") of it, in total, the pet
is no longer #w("diabetic"). The count does not fall when the pet slips. A
new onset starts it again from zero. Exercise and #w("Ozempic") take weight
off. Ozempic also takes fitness off, so it is a mixed blessing here.


== Well rested

*How it starts.* While asleep, settled and short of full energy, the pet counts
*good sleep*. Settled means none of the reasons under #w("bad night") below
applies. If it wakes with at least
#v("GOOD_SLEEP_LINE_MIN") of it, it is #w("well rested") for
#v("RESTED_MIN"). The toast is #w("well rested") with #w("good sleep"). It is
the one toast that is good news. After a bad night the pet is still well
rested if it slept enough good minutes. Only the toast changes: it says
#w("bad night") instead.

*What it does.* Exercise strains #v("DOSE_STRAIN_RESTED_PCT") as hard. The
status line shows #w("well rested") when nothing worse is going on. It has
no mood bonus.

*How to keep it.* Sleep fed, watered and sober. Every reason in the
#w("bad night") list below stops the count.

== The boot alarms

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    Diabetes and dependence each have an *alarm*: a buzz, and a full-screen
    alert. The alert takes over the whole display, even from a menu or a
    mini-game, for at least #v("ALERT_VISIBLE_SECS"). Diabetes reads
    #w("TYPE 2"), #w("DIABETES"), and in red #w("Give medication soon").
    Dependence reads #w("ALCOHOL"), #w("DEPENDENCE"), and in red
    #w("Give rehab soon").

    *The alarm sounds again on every boot*, for as long as the pet has the
    condition. It does not matter that you have already given insulin. The
    alarm does not even stop for a pet in rehab. The one exception is
    *hibernation*: a pet that hibernates is left in peace. If the alarm has
    not sounded yet this boot, it sounds when you defrost the pet.
  ],
  screen("alert-diabetes", width: shot-s, caption: [The diabetes alert.]))

The alarm sounds once per boot, not every minute. The mute switch (@ch-reading)
silences the buzz, but not the screen: the alert still shows. A pet that
loses the condition and gets it again sounds the alarm again.

#joke[The badge has no snooze button for this. It says the pet is diabetic
every time you turn it on. It is a very caring badge. Also a bit of a nag.]

== Every cause toast

When a dose or a night goes badly, the badge tells you why, in two red lines.
The first line is the *what*. The second is the *why*. The diary keeps the
last ones (@ch-reading). This is every one:

#set text(size: 8.5pt)
#toast-table(
  [#w("food coma") #linebreak() #w("ate too much")], [A meal took the belly past #v("FULL_HI_PCT").], [Wait. The excess goes to weight and energy.],
  [#w("barely felt") #linebreak() #w("treat too soon")], [A treat inside #v("DOSE_JUNK_MIN") of the last one.], [Space treats out.],
  [#w("no buzz") #linebreak() #w("on Ozempic")], [A treat while Ozempic works.], [Wait for it to wear off.],
  [#w("barely felt") #linebreak() #w("drunk already")], [A drink for a pet with more than half a bar of booze (any booze at all if the pet is #w("dependent")).], [Stop. Give water.],
  [#w("sloppy") #linebreak() #w("one too many")], [A drink took booze over #v("BOOZE_HI_PCT").], [Water.],
  [#w("dry mouth") #linebreak() #w("booze, no water")], [A drink with water under #v("WATER_LO_PCT").], [Water first.],
  [#w("sloshing") #linebreak() #w("drank too much")], [A drink pushed water over #v("WATER_HI_PCT") or the belly over #v("FULL_HI_PCT").], [Let it settle.],
  [#w("relapse!") #linebreak() #w("rehab lost")], [Alcohol during rehab.], [Start #w("Rehab") again.],
  [#w("bored") #linebreak() #w("too much relax")], [Relax took fun from the band to under #v("FUN_LO_PCT").], [Play.],
  [#w("only numbed") #linebreak() #w("not sick")], [Aspirine for a healthy pet: health over #v("DOSE_HEAL_HEALTHY_PCT"), no injury. Only adds booze.], [Keep Aspirine for the hurt.],
  [#w("torn muscle") #linebreak() sport reason], [Strain over #v("STRAIN_INJ_PCT") after exercise. The reason is the first handicap in the list above, else #w("sport: too much").], [Rest. Aspirine helps.],
  [#w("strained") #linebreak() sport reason], [A session with one of those handicaps, but no tear.], [Fix the handicap first.],
  [#w("grumpy") #linebreak() #w("woken too early")], [Wake up by hand with energy under #v("GRUMPY_ENERGY_PCT").], [Let it sleep.],
  [#w("groggy") #linebreak() #w("overslept")], [Woke after more than #v("GROGGY_LINE_MIN") of oversleeping.], [Coffee, water or play.],
  [#w("perked up") #linebreak() #w("had a coffee")], [A coffee ended a groggy spell.], [Nothing. Enjoy.],
  [#w("wired") #linebreak() #w("too much coffee")], [Cup number #v("COFFEE_WIRED_CUPS") inside #v("COFFEE_WINDOW_MIN").], [Relax. No more coffee for a while.],
  [#w("worn out") #linebreak() #w("short nights")], [The short-night run reached #v("DEPRIVED_NIGHTS").], [Sleep long.],
  [#w("hung over") #linebreak() #w("needs water")], [Woke while the hangover still runs.], [Water.],
  [#w("bad night") #linebreak() a reason], [At least #v("RESTLESS_MIN") of poor sleep, and more than half the night. Why: see below.], [Fix the reason before bed.],
  [#w("well rested") #linebreak() #w("good sleep")], [Woke with #v("GOOD_SLEEP_LINE_MIN") of good sleep.], [Nothing. Enjoy.],
)

#set text(size: 10pt)
#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Why a night went badly.* The second line of #w("bad night") is the first
    reason that fits, as the pet lay down or woke up:

    #set text(size: 9pt)
    #word-table(see: false,
      table.header([*Reason*], [*It means*]),
      [#w("late drinks")], [Booze over #v("UNSETTLED_BOOZE_PCT").],
      [#w("hungry")], [Belly under #v("UNSETTLED_FULL_LO_PCT").],
      [#w("stuffed")], [Belly over #v("UNSETTLED_FULL_HI_PCT").],
      [#w("thirsty")], [Water under #v("UNSETTLED_WATER_LO_PCT").],
      [#w("waterlogged")], [Water over #v("UNSETTLED_WATER_HI_PCT").],
      [#w("wired")], [Fun over #v("FUN_HI_PCT").],
      [#w("withdrawal")], [A rehab is running.],
      [#w("slept badly")], [No reason is left by the time of waking.],
    )
  ],
  screen("cause-toast", width: shot-s, caption: [A cause toast.]))
