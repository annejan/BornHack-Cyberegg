#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/mood.typ": *
#chapter("ch-mood")[Mood and health]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

Two things decide whether your pet stays. *Mood* is how it feels. *Health*
is how long it can stay. The nine needs of @ch-needs feed both, and this
chapter shows how. It ends with the sad part: what happens when health runs
out.

== The big picture

#mood-flow()

Mood chases a *target*. The target starts at full. Every need that sits
outside its comfort band, and every lasting condition, pulls it down. The
mood then walks to the target, one small step a minute. Health looks at the
mood, at the needs and at fitness. Nothing here is random. The same pet in
the same state always gets the same result.

== Mood

*The target.* A need outside its band pulls the target down by a share of how
far outside it is. The share depends on the need. *Lasting conditions* pull by
a flat amount, as long as they last.

#set text(size: 9pt)
#set par(justify: false)
#grid(columns: (1.4fr, 1fr), column-gutter: 4mm, align: top,
  pull-table(
    table.header([*Per point outside*], [*Needs*]),
    [One and a half], [Belly #under("FULL_LO_PCT"), water #under("WATER_LO_PCT"),
      energy #under("ENERGY_LO_PCT") awake, fun #over("FUN_HI_PCT")
      (#w("wired")).],
    [One], [Belly #over("FULL_HI_PCT") (#w("stuffed")), water #over("WATER_HI_PCT")
      (#w("waterlogged")), fun #under("FUN_LO_PCT") awake (#w("bored")),
      booze #over("BOOZE_HI_PCT") (#w("sloppy")), dirt #over("DIRT_HI_PCT")
      (#w("grubby")), health #under("MOOD_HEALTH_FLOOR_PCT") (#w("poorly")).],
    [Half], [Weight #over("WEIGHT_HI_PCT") (#w("overweight")), weight
      #under("WEIGHT_LO_PCT") (#w("underweight")), fitness #under("FIT_LO_PCT")
      (#w("unfit")).],
  ),
  flat-table(
    table.header([*Condition*], [*Flat pull*]),
    [#w("hung over")], [#v("MOOD_PEN_HANGOVER_PCT")],
    [#w("injured")], [#v("MOOD_PEN_INJURED_PCT")],
    [#w("groggy")], [#v("MOOD_PEN_GROGGY_PCT")],
    [#w("worn out")], [#v("MOOD_PEN_DEPRIVED_PCT")],
    [#w("in rehab")], [#v("MOOD_PEN_REHAB_PCT")],
    [Craving \*], [#v("MOOD_PEN_CRAVING_PCT")],
  ))
#set text(size: 10pt)
#set par(justify: true)

\* Craving: dependent, awake, booze under #v("MOOD_CRAVING_BOOZE_PCT"), not in
rehab.

The pulls add up. The target never goes under empty.

*The walk.* Mood moves toward the target by at most one step a minute. A
full sweep of the bar takes #sweep("MOOD_STEP"). So a bad hour dents the mood
and a bad day wrecks it. Treats and drinks also nudge the mood directly, see
@ch-care.

#tip[Mood is not a need, so it has no comfort band. Its bar is on the second
stats page. The #w("Mood") bar goes red under #v("DMG_MOOD_PCT"). That is the
line where it starts to hurt health.]

== The health ceiling

Health cannot climb past a *ceiling*. Fitness sets it, in four steps:

#ceiling-bar()

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    A *diabetic pet without insulin* loses #v("CEILING_DIABETIC_PCT") points
    from its ceiling. A dose of insulin gives them back for as long as it
    lasts. @ch-conditions tells how a pet gets there.

    If the ceiling drops under the health the pet has, health sinks to it at
    a gentle pace: a whole bar in #sweep("HEALTH_EXCESS_DECAY_PM").

    The second stats page draws the ceiling as a tick on the #w("Health")
    bar. A fit pet has room to heal. A couch pet has not.
  ],
  screen("stats-body", width: shot-m, caption: [#w("Body 2/2"): the ceiling tick.]))

== Damage

Health drains whenever a need is far out. The terms add up. Each line below is
what the drain does *alone*: the time it takes to empty a full health bar.
Casual drains at half the speed.

#dmg-table(
  table.header([*What hurts*], [*Line*], [*Classic*], [*Casual*]),
  ..dmg-row([Belly empty], [under #v("DMG_NEED_LOW_PCT")], "DMG_FULL_LOW_PM"),
  ..dmg-row([Belly packed], [over #v("DMG_NEED_HIGH_PCT")], "DMG_FULL_HIGH_PM"),
  ..dmg-row([Water empty], [under #v("DMG_NEED_LOW_PCT")], "DMG_WATER_LOW_PM"),
  ..dmg-row([Water packed], [over #v("DMG_NEED_HIGH_PCT")], "DMG_WATER_HIGH_PM"),
  ..dmg-row([Awake and spent], [energy under #v("DMG_ENERGY_PCT")], "DMG_ENERGY_PM"),
  ..dmg-row([Wasted], [booze over #v("BOOZE_DANGER_PCT")], "DMG_BOOZE_PM"),
  ..dmg-row([Miserable], [mood under #v("DMG_MOOD_PCT")], "DMG_MOOD_PM"),
  ..dmg-row([Filthy], [dirt over #v("DMG_DIRT_PCT")], "DMG_DIRT_PM"),
  ..dmg-row([Too heavy], [weight over #v("DMG_WEIGHT_HIGH_PCT")], "DMG_WEIGHT_HIGH_PM"),
  ..dmg-row([Too thin], [weight under #v("DMG_WEIGHT_LOW_PCT")], "DMG_WEIGHT_LOW_PM"),
)

Every line is strict: *at* the line does not hurt, past it does. Lack of
sleep alone cannot finish a pet. The tiredness drain stops once health is
down to #v("DMG_ENERGY_HEALTH_PCT"). Of the two weight lines, too thin is the cruel one and too heavy the kind one.

== Recovery

Health heals only while it is under the ceiling, and only when the mood is
good:

#grid(columns: (1fr, auto), column-gutter: 5mm, align: horizon,
  [
- Mood from #v("REC_GOOD_MOOD_PCT"): a full bar in #sweep("REC_GOOD_PM").
- Mood from #v("REC_FAIR_MOOD_PCT"): a full bar in #sweep("REC_FAIR_PM").
- Mood under that: no healing at all.
- A #w("worn out") pet heals at #v("REC_DEPRIVED_PCT") of that speed.
  ],
  hero-pic("healing"))

Damage and recovery work in the same minute. The pet gains the difference.
Healing is slow. Damage can be quick. Keep the needs in their bands and
neither matters.


== Collapse

A pet whose energy reaches empty while awake *collapses*. It falls asleep on
the spot, as if it had gone to bed for the night. If its health is over
#v("COLLAPSE_HEALTH_FLOOR_PCT"), the fall also costs
#raw-pct("COLLAPSE_DAMAGE") of health (#raw-pct("COLLAPSE_DAMAGE", preset: casual) in Casual).
Under that line the fall costs nothing: a pet that low is already in enough
trouble.

== Leaving and Gone

#grid(columns: (1fr, auto, auto), column-gutter: 4mm, align: top,
  [
    When health hits zero, the pet is *leaving*. The picture shows a packed
    suitcase. The status line keeps its needs words. The leaving clock runs
    for #v("LEAVING_MIN"), counted only while health stays at zero. The
    buzzer gives a warning when the pet steps onto this rung.

    *Save it.* Anything that lifts health above zero stops the clock. The
    clock then runs back at half speed. When it reaches zero the pet is back
    to normal. Feed, water and sleep fix the cause. #w("Aspirine") from
    #w("Heal") gives a sick pet #v("DOSE_HEAL_HEALTH_PCT") of health, if its health is
    #v("DOSE_HEAL_HEALTHY_PCT") or less and it is not injured. After a dose
    the row says #w("no need") for #v("DOSE_MED_CD_MIN").
  ],
  screen("leaving", width: shot-s, caption: [Leaving.]),
  screen("gone", width: shot-s, caption: [Gone.]))

*Gone.* If the clock runs out, the pet leaves for good. The badge saves that
fact and plays its short, silly tune (the mute switch silences it). The
screen shows a farewell sign, or the words #w("Your pet has left") if the
pet has no farewell picture. Below it, in red, the badge says #w("Press Fire")
and #w("for a new egg"). Fire starts the next egg. The pet that left joins
the others in the Unicorn Realm, see @ch-realm.

Gone is final. A leaving pet is *not* gone yet: while the clock runs, you can
still save it. #w("poorly") is the danger word for low health, under
#v("HEALTH_DANGER_PCT"). The #w("Hint") row names the fix.
