#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#chapter("ch-needs")[The nine needs]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

Your pet has nine needs: fullness, water, energy, fun, dirt, booze, weight,
fitness and strain. Each is a level from empty to full. Each has a *comfort
band*, a stretch where the pet is fine.

#block[
    - *Inside the band*, nothing happens. The pet is calm.
    - *Outside it*, the need pulls the pet's mood down. The further out, the
      harder the pull. The status line may name the problem.
    - *Far outside*, the pet's health pays, and some words turn red.
    - *On the edge itself* still counts as inside. Only past the edge hurts.
]

#pic-row((("idle", [In the band.]), ("hungry", [Past an edge.]), ("sick", [Far past it.])))

The needs move by themselves, minute by minute. Feeding, drinking, playing
and the rest are *doses* that push a level up or down in one go.
@ch-care shows every action. @ch-mood shows how the pulls add up to a mood.
This chapter takes the needs one at a time, one page each, and ends with the
stats view that shows them all.

== The nine at a glance

Classic is the guide's number. Casual moves each edge a few points.

#dose-table((auto, auto, 1fr, 1fr),
  table.header([*Need*], [*Stats view*], [*Classic*], [*Casual*]),
  ..edge-row("Full", [#w("Full")], "band", "FULL_LO_PCT", hi: "FULL_HI_PCT"),
  ..edge-row("Water", [#w("Water")], "band", "WATER_LO_PCT", hi: "WATER_HI_PCT"),
  ..edge-row("Energy", [#w("Energy")], "from", "ENERGY_LO_PCT"),
  ..edge-row("Fun", [#w("Fun")], "band", "FUN_LO_PCT", hi: "FUN_HI_PCT"),
  ..clean-row(),
  ..edge-row("Booze", [#w("Booze")], "upto", "BOOZE_HI_PCT"),
  ..edge-row("Weight", [#w("Weight")], "band", "WEIGHT_LO_PCT", hi: "WEIGHT_HI_PCT"),
  ..edge-row("Fitness", [#w("Fit")], "from", "FIT_LO_PCT"),
  ..edge-row("Strain", [not shown], "upto", "STRAIN_INJ_PCT"),
)

#pagebreak(weak: true)
== How to read the bars

Each need page has a bar from empty on the left to full on the right. Under
every zone is its name, and where it starts or ends.

#bar-key()

#block(above: 3mm)[
  Most zone names are the status words of @ch-reading. "Over" and "under"
  leave the edge out. "To" and "from" keep it in. The *hurts* zones are where
  the pet loses health, minute after minute.
]

#pic-row((("hungry", [Peckish, then hungry.]), ("tired", [Sleepy.]), ("critical", [Past the damage line.])))

*How fast do they fall?* Take a pet in the middle of every band, awake, left
alone. It turns #w("peckish") after #until("peckish"), #w("thirsty") after
#until("thirsty"), #w("sleepy") after #until("sleepy") and #w("bored") after
#until("bored"). Dirt is much slower: such a pet does not turn #w("grubby")
for days.

*How hard do they pull?* A need that is outside its band pulls the pet's
mood *target* down. @ch-mood explains the target. The pull grows with the
distance past the edge:

#dose-table((30%, 1fr),
  table.header([*Mood lost per point past the edge*], [*Words*]),
  [A point and a half], [#w("hungry"), #w("thirsty"), #w("sleepy") and #w("wired")],
  [One point], [#w("stuffed"), #w("waterlogged"), #w("bored"), #w("sloppy") and #w("grubby")],
  [Half a point], [#w("overweight"), #w("underweight") and #w("unfit")],
)

A pet only a hair past an edge pulls too little to be named. The status line
waits until the pull is a few points.

#tip[A pet can sit in the comfort zone of eight needs and still sulk about
the ninth. Check them one at a time.]

#pagebreak(weak: true)
== Full

*Full* is how full the belly is. It is the need you will meet most.

#need-bar("full")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Down.* An awake pet empties its belly all the time. A sleeping pet
    empties it more slowly. Play costs #dose-pct("Play", "full") and
    exercise costs #dose-pct("Exercise", "full").

    *Up.* Feed it. Food does not stop at the top of the band. The extra,
    counted up to #v("DOSE_FULL_CAP_PCT"), turns into weight and costs
    energy. The toast says #w("food coma"), because the pet #w("ate too much").
    Drinks fill the belly a little too.
  ],
  screen("starving", width: shot-m, caption: [Under the line.]))

#dose-table((auto, 1fr, 1fr, 1fr, 1fr),
  table.header([*Food*], [*Full*], [*Dirt*], [*Weight*], [*Mood*]),
  [#w("Salad")], dose-pct("Salad", "full"), dose-pct("Salad", "dirt"), [none], [none],
  [#w("Apple")], dose-pct("Apple", "full"), dose-pct("Apple", "dirt"), dose-pct("Apple", "weight"), [none],
  [#w("Frikandel spec")], dose-pct("Frikandel spec", "full"), dose-pct("Frikandel spec", "dirt"), dose-pct("Frikandel spec", "weight"), dose-pct("Frikandel spec", "mood"),
  [#w("Pizza")], dose-pct("Pizza", "full"), dose-pct("Pizza", "dirt"), dose-pct("Pizza", "weight"), dose-pct("Pizza", "mood"),
  [#w("Cake")], dose-pct("Cake", "full"), dose-pct("Cake", "dirt"), dose-pct("Cake", "weight"), dose-pct("Cake", "mood"),
)

Every food adds dirt. Salad and apple also give the pet
#dose-pct("Salad", "water") of water, and salad adds
#dose-pct("Salad", "fit") of fitness. A treat gives its full mood only if the
last treat is more than #v("DOSE_JUNK_MIN") old, and none at all on Ozempic.

*Past the edges.*
- Under #v("FULL_LO_PCT") the pet is #w("peckish"). Under #v("HUNGRY_PCT") it
  is #w("hungry"). Each point under pulls a point and a half off the mood.
- Under #v("NEED_DANGER_LO_PCT") it is #w("starving") and the status line
  turns red. Under #v("DMG_NEED_LOW_PCT") health drains. A belly under #v("UNSETTLED_FULL_LO_PCT") already
  spoils the night's sleep.
- Over #v("FULL_HI_PCT") it is #w("stuffed"). Each point over pulls a point
  off the mood, and a belly over #v("UNSETTLED_FULL_HI_PCT") also spoils the
  sleep. Over #v("DMG_NEED_HIGH_PCT") health drains, gently.
- A hungry pet also strains more at exercise.

#joke[Peckish, hungry, starving: the pet has three words for "not enough".
It has one for "too much", and it says it with feeling.]

#pagebreak(weak: true)
== Water

*Water* is hydration. It has a band of its own, a little wider than the
belly's.

#need-bar("water")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Down.* An awake pet loses water all the time, a sleeping pet more
    slowly. Booze makes it worse. From #v("THIRST_BOOZE_LOW_PCT") booze the
    pet dries out at #v("THIRST_MULT_LOW_PCT") of the pace, from
    #v("THIRST_BOOZE_MID_PCT") at #v("THIRST_MULT_MID_PCT"), and from
    #v("THIRST_BOOZE_HIGH_PCT") at #v("THIRST_MULT_HIGH_PCT"). Play costs
    #dose-pct("Play", "water") and exercise #dose-pct("Exercise", "water").

    *Up.* Give it a drink. Water gives up to #v("DOSE_WATER_MAX_PCT"), but
    never fills past #v("WATER_CAP_PCT"). A pet with water over
    #v("WATER_REFUSE_PCT") says #w("not thirsty") and turns the glass down.
  ],
  hero-pic("drinking"))

#dose-table((auto, 1fr, 1fr),
  table.header([*Drink*], [*Water*], [*Full*]),
  [#w("Water")], dose-pct("Water", "water"), dose-pct("Water", "full"),
  [#w("Coffee")], dose-pct("Coffee", "water"), dose-pct("Coffee", "full"),
  [#w("Beer")], dose-pct("Beer", "water"), dose-pct("Beer", "full"),
  [#w("Wine")], dose-pct("Wine", "water"), dose-pct("Wine", "full"),
  [#w("Whiskey")], dose-pct-or("Whiskey", "water", [none]), dose-pct-or("Whiskey", "full", [none]),
)

Plain water does more than wet the pet. It lowers booze by
#v("DOSE_WATER_BOOZE_PCT"), shortens a hangover by
#v("DOSE_WATER_HANGOVER_MIN") and a groggy spell by
#v("DOSE_WATER_GROGGY_MIN"). A dry pet also clears booze more slowly, at only
#v("BOOZE_CLEAR_SLOW_PCT") of the normal speed.

*Past the edges.*
- Under #v("WATER_LO_PCT") the pet is #w("thirsty"). Each point under pulls a
  point and a half off the mood.
- Under #v("NEED_DANGER_LO_PCT") it is #w("parched") and the line turns red.
  Under #v("DMG_NEED_LOW_PCT") health drains.
  Under #v("UNSETTLED_WATER_LO_PCT") the sleep is already poor.
- Over #v("WATER_HI_PCT") it is #w("waterlogged"). Each point over pulls a
  point off the mood. Over #v("UNSETTLED_WATER_HI_PCT") the sleep suffers too,
  and over #v("DMG_NEED_HIGH_PCT") health drains, gently.
- A drink that tips the pet over the top toasts #w("sloshing").

#tip[Water has no booze and no mood bonus. It is plain, it is safe, and in
moderation it fixes most things. Even a hangover.]

#pagebreak(weak: true)
== Energy

*Energy* is not a band. It has a *floor*: a line to stay above, and no top
edge. A pet with plenty of energy is never in trouble for it.

#need-bar("energy")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Down.* An awake pet burns energy. Play costs
    #dose-pct("Play", "energy"), exercise #dose-pct("Exercise", "energy").
    Some states burn it faster, as a share of the normal pace: an
    #w("unfit") pet at #v("ENERGY_MULT_UNFIT_PCT"), a #w("sloppy") one at
    #v("ENERGY_MULT_DRUNK_PCT"), a #w("wired") one at
    #v("ENERGY_MULT_WIRED_PCT"), a #w("hung over") one at
    #v("ENERGY_MULT_HANGOVER_PCT"), and a pet with a weight problem at
    #v("ENERGY_MULT_WEIGHT_PCT"). A fit pet, from #v("ENERGY_FIT_GOOD_PCT")
    fitness up, burns it at #v("ENERGY_MULT_FIT_PCT").

    *Up.* Sleep refills it. Coffee gives #dose-pct("Coffee", "energy") (more
    to a groggy pet) and relaxing gives #dose-pct("Relax", "energy").
  ],
  hero-pic("tired"))

*Poor sleep.* A pet that is hungry, stuffed, thirsty, waterlogged, wired,
in withdrawal or drunk over #v("UNSETTLED_BOOZE_PCT") sleeps badly and
refills more slowly. A #w("worn out") pet cannot refill above
#v("ENERGY_CAP_DEPRIVED_PCT"). @ch-care explains sleep, and @ch-conditions
explains #w("worn out").

*Past the edge.*
- Under #v("ENERGY_LO_PCT") the pet is #w("sleepy"), but only while it is
  awake. A sleeping pet has no mood pull from this. Each point under pulls a
  point and a half off the mood.
- Under #v("EXHAUSTED_PCT") it is #w("exhausted"). The buzzer sounds. The
  status line does not turn red; the red zone on the bar is the damage zone.
  Under #v("DMG_ENERGY_PCT"), while the pet is awake, health drains. The drain stops once health is
  down to #v("DMG_ENERGY_HEALTH_PCT"): lack of sleep alone does not finish
  a pet.
- A tired pet gets double fun from play, takes harder drinks, and strains
  more at exercise. #w("Only pets") turns it down: #w("too tired").
- Wake it with energy under #v("GRUMPY_ENERGY_PCT") and it is #w("grumpy"):
  the mood drops by #v("GRUMPY_PCT").
- Put it to bed with energy at #v("ENERGY_HI_EDGE_PCT") or more and it is
  not tired. The badge warns that it will wake #w("groggy").

#joke[Sleepy and exhausted are two words on the badge. The pet thinks they
mean the same: "now".]

#pagebreak(weak: true)
== Fun

*Fun* is a band with two edges. A bored pet sulks. An overexcited pet is just
as hard to live with.

#need-bar("fun")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Down.* Fun leaks away all the time. A sleeping pet's fun stops falling
    at #v("SLEEP_FUN_FLOOR_PCT"): it never sleeps itself bored. Relaxing
    costs #dose-pct("Relax", "fun"). It is a rest, not a party.

    *Up.* Play gives #dose-pct("Play", "fun"), and a tired pet gets double.
    A mini-game gives at least #v("DOSE_MINI_FUN_BASE_PCT"), and
    #v("DOSE_MINI_FUN_GRADE_PCT") more for each grade above the lowest.
    Exercise gives #dose-pct("Exercise", "fun"), and an alcoholic drink gives
    #dose-pct("Beer", "fun"). Meeting a friend gives
    #v("DOSE_FRIEND_BIG_FUN_PCT") for a new one and
    #v("DOSE_FRIEND_SMALL_FUN_PCT") for a known one.
  ],
  hero-pic("playing"))

#dose-table((auto, 1fr, 1fr),
  table.header([*Action*], [*Fun*], [*Energy*]),
  [#w("Play")], dose-pct("Play", "fun"), dose-pct("Play", "energy"),
  [#w("Exercise")], dose-pct("Exercise", "fun"), dose-pct("Exercise", "energy"),
  [#w("Relax")], [minus #dose-pct("Relax", "fun")], [plus #dose-pct("Relax", "energy")],
  [#w("Only pets")], dose-pct("OnlyPets", "fun"), [minus #dose-pct("OnlyPets", "energy")],
)

*Past the edges.*
- Under #v("FUN_LO_PCT") the pet is #w("bored"), but only while awake. Each
  point under pulls a point off the mood. There is no health damage and no
  danger word. A bored pet is only unhappy.
- Over #v("FUN_HI_PCT") it is #w("wired"). Each point over pulls a point and
  a half off the mood. A wired pet burns energy faster and sleeps badly. This
  pull works while the pet sleeps too. Too much coffee wires a pet as well,
  see @ch-conditions.
- Relax too hard and the toast says #w("too much relax"): the pet ended up
  #w("bored").

#tip[A pet that is too tired to play gets twice the fun out of it. Cruel, but
effective.]

#pagebreak(weak: true)
== Dirt

*Dirt* is the need that is easiest to read the wrong way round. The bar below
runs from spotless on the left to filthy on the right. The stats view shows
the same need as *Clean*, upside down: a pet at #clean-line() clean is at
#v("DIRT_HI_PCT") dirt, the edge of its band.

#need-bar("dirt")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Up.* Dirt creeps up slowly by itself. The real mess comes from what the
    pet does. Every meal adds dirt, as the food table showed. Play adds
    #dose-pct("Play", "dirt") and exercise adds #dose-pct("Exercise", "dirt").

    *Down.* Wash. One wash takes off #dose-pct("Wash", "dirt") and does
    nothing else. A pet with dirt under #v("WASH_REFUSE_PCT") says
    #w("already clean"), and the wash does not happen.
  ],
  hero-pic("sick"))

*Past the edges.*
- Over #v("DIRT_HI_PCT") the pet is #w("grubby"). Each point over pulls a
  point off the mood.
- Over #v("DIRT_DANGER_PCT") it is #w("filthy"). The status line turns red.
- Over #v("DMG_DIRT_PCT") health drains. In Classic the line and the damage
  sit at the same place, but they are two settings.

Dirt has no lower edge. A spotless pet is never a problem. It does not stay
spotless for long.

#warn[Do not read the Clean bar like the other bars. A short Clean bar is a
dirty pet. Red on Clean means the pet needs a wash.]

#joke[The pet never thinks it is dirty. It thinks it is "lived in".]

#pagebreak(weak: true)
== Booze

*Booze* is how drunk the pet is. Its band runs from sober up to
#v("BOOZE_HI_PCT"). There is no bottom edge to worry about.

#need-bar("booze")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Up.* A drink adds booze. A tired pet takes #v("DOSE_TIRED_BOOZE_PCT") of
    it. A drunk pet also gets less mood and energy out of the glass. The
    toast says #w("barely felt") or #w("drunk already").

    *Down.* Booze clears by itself, slowly. A dry pet and a dependent pet
    each clear it at only #v("BOOZE_CLEAR_SLOW_PCT") of the pace, and both
    together stack. Water takes off #v("DOSE_WATER_BOOZE_PCT") and exercise
    takes off #v("DOSE_EXERCISE_BOOZE_PCT"). Medicine from #w("Heal") adds
    #v("DOSE_MED_BOOZE_PCT") to an injured or a healthy pet: it numbs.
  ],
  screen("wasted", width: shot-m, caption: [Over the line.]))

#dose-table((auto, 1fr, 1fr, 1fr, 1fr),
  table.header([*Drink*], [*Booze*], [*Energy*], [*Fun*], [*Mood*]),
  [#w("Beer")], dose-pct("Beer", "booze"), dose-pct("Beer", "energy"), dose-pct("Beer", "fun"), dose-pct("Beer", "mood"),
  [#w("Wine")], dose-pct("Wine", "booze"), dose-pct("Wine", "energy"), dose-pct("Wine", "fun"), dose-pct("Wine", "mood"),
  [#w("Whiskey")], dose-pct("Whiskey", "booze"), dose-pct("Whiskey", "energy"), dose-pct("Whiskey", "fun"), dose-pct("Whiskey", "mood"),
)

*Past the edges.*
- From #v("TIPSY_PCT") the pet is #w("tipsy"). That is only an info word. The
  pet is still inside the band.
- Over #v("BOOZE_HI_PCT") it is #w("sloppy"). Each point over pulls a point
  off the mood, and the pet burns energy faster. Crossing the line toasts
  #w("one too many"). Later, when the booze falls under
  #v("BOOZE_CLEAR_PCT"), a #w("hung over") spell starts and lasts
  #v("HANGOVER_MIN").
- Over #v("BOOZE_DANGER_PCT") it is #w("wasted"): red, and health drains.
- Booze over #v("UNSETTLED_BOOZE_PCT") spoils sleep. A long habit over the
  band leads to dependence. @ch-conditions has the rest.

#joke[A tipsy pet is cheerful and harmless. The line between tipsy and sloppy
is the line between a good night and a bad toast.]

#pagebreak(weak: true)
== Weight

*Weight* counts both ways. A pet can be too heavy and a pet can be too light.
Both cost mood and energy.

#need-bar("weight")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Up.* Food adds weight, as the food table showed. Beer adds
    #dose-pct("Beer", "weight"). A belly over #v("FULL_HI_PCT") pushes weight
    up all the time, and so does a food coma.

    *Down.* Exercise takes off #dose-pct("Exercise", "weight"). An empty belly,
    under #v("FULL_LO_PCT"), pulls weight down. Metabolism pulls it down
    gently, faster when the pet is too heavy, and not at all when the pet is
    too light. Ozempic takes off #dose-pct("Ozempic", "weight"), once every
    #v("DOSE_OZEMPIC_CD_MIN"), and costs #dose-pct("Ozempic", "fit") of
    fitness.
  ],
  hero-pic("happy"))

*Past the edges.*
- Over #v("WEIGHT_HI_PCT") the pet is #w("overweight"). Under
  #v("WEIGHT_LO_PCT") it is #w("underweight"). Each point past either edge
  pulls half a point off the mood.
- Out of the band on either side, energy burns at
  #v("ENERGY_MULT_WEIGHT_PCT") of the pace.
- Over #v("WEIGHT_HI_PCT") fitness also fades faster.
- Over #v("DMG_WEIGHT_HIGH_PCT") health drains, slowly. Under
  #v("DMG_WEIGHT_LOW_PCT") it drains much faster. Too thin is worse than too
  round.
- A pet that stays over #v("DIABETES_WEIGHT_HI_PCT") for a long time can
  become diabetic. @ch-conditions has the story.

#tip[Weight pulls the mood gently, so its word is seldom the loudest on the
status line. Check the #w("Weight") bar in the stats view.]

#pagebreak(weak: true)
== Fitness

*Fitness* has a floor and no top. More is always fine.

#need-bar("fitness")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Up.* Exercise gives #dose-pct("Exercise", "fit"). Play gives
    #dose-pct("Play", "fit"). Salad gives #dose-pct("Salad", "fit"). A torn
    muscle gets no fitness at all.

    *Down.* Fitness fades by itself, and the fitter the pet, the faster it
    fades: quickest above #v("FIT_DECAY_HIGH_ABOVE_PCT"), slower above
    #v("FIT_DECAY_MID_ABOVE_PCT") and slowest below that. An #w("overweight")
    pet loses it at #v("FIT_DECAY_HEAVY_PCT") of the pace. Relaxing costs
    #dose-pct("Relax", "fit"), Ozempic #dose-pct("Ozempic", "fit"), and a
    groggy wake-up #v("GROGGY_FIT_PCT").
  ],
  hero-pic("exercising"))

Fitness is also the roof on health. The ceiling steps up with fitness, and
an untreated diabetic pet loses #v("CEILING_DIABETIC_PCT") of it.
@ch-mood has the story.

#dose-table((1fr, 1fr),
  table.header([*Fitness*], [*Health can heal up to*]),
  [Under #v("CEILING_FIT_STEP1_PCT")], v("CEILING_STEP0_PCT"),
  [Under #v("CEILING_FIT_STEP2_PCT")], v("CEILING_STEP1_PCT"),
  [Under #v("CEILING_FIT_STEP3_PCT")], v("CEILING_STEP2_PCT"),
  [#v("CEILING_FIT_STEP3_PCT") and up], v("CEILING_STEP3_PCT"),
)

*Past the edge.*
- Under #v("FIT_LO_PCT") the pet is #w("unfit"). Each point under pulls half
  a point off the mood, and energy burns at #v("ENERGY_MULT_UNFIT_PCT") of the
  pace.
- From #v("ENERGY_FIT_GOOD_PCT") up, energy burns at only
  #v("ENERGY_MULT_FIT_PCT"). Exercise strains less too, from
  #v("DOSE_STRAIN_FIT_MIN_PCT") up.
- A diabetic pet can shake the condition off, if its weight stays under
  #v("DIABETES_WEIGHT_LO_PCT") and its fitness over #v("REVERSAL_FIT_PCT")
  for long enough. @ch-conditions has the details.

#joke[Fitness has no top edge to worry about. The pet is still not sure why
it has to go jogging.]

#pagebreak(weak: true)
== Strain

*Strain* is how hard the muscles have worked. It is the odd one out: it has
no comfort band, only a line you must not cross. It is also the one need the
stats view does not show.

#need-bar("strain")

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *Up.* Only exercise adds strain. A plain session adds
    #v("DOSE_EXERCISE_STRAIN_PCT"). The pet's state scales that: to
    #v("DOSE_STRAIN_NEED_PCT") if it is hungry, thirsty or tired, to
    #v("DOSE_STRAIN_HEAVY_PCT") if it is #w("overweight"), to
    #v("DOSE_STRAIN_DEPRIVED_PCT") if it is #w("worn out"), to
    #v("DOSE_STRAIN_RESTED_PCT") if it is #w("well rested"), and to
    #v("DOSE_STRAIN_FIT_PCT") from #v("DOSE_STRAIN_FIT_MIN_PCT") fitness up.

    *Down.* It clears by itself, over some hours. Relaxing takes off
    #dose-pct("Relax", "strain").
  ],
  screen("injured", width: shot-m, caption: [A torn muscle.]))

*Past the edges.*
- Over #v("SORE_PCT") the pet is #w("sore"). It is an info word and has no
  mood pull. It is your warning: another session now may tear a muscle. When
  an injury is already running, the pet says #w("injured") instead.
- A session that ends over #v("STRAIN_INJ_PCT") tears a muscle. The pet is
  #w("injured") for #v("DOSE_INJURY_MIN"), health drops by
  #v("DOSE_INJURY_HEALTH_PCT"), and the mood loses #v("MOOD_PEN_INJURED_PCT")
  for as long as it lasts. That session gives no fitness.
- An injured pet turns down play, exercise and #w("Only pets"), saying
  #w("injured"). Medicine from #w("Heal") takes #v("DOSE_HEAL_INJURY_MIN") off
  the injury. @ch-conditions has the full story.

#tip[Look at the status line before you exercise. A #w("sore") pet is one
session away from #w("injured").]

#pagebreak(weak: true)
== The stats view

Everything above shows in one place: the *stats view*. Pick the Stats icon,
then #w("View stats"). The view has two pages. Press Left or Right to flip
between them. Fire or EXE goes back to the list, and CAN closes the menu.

#grid(columns: (auto, auto, 1fr), column-gutter: 4mm, align: top,
  screen("stats-needs", width: shot-s, caption: [#w("Needs 1/2").]),
  screen("stats-body", width: shot-s, caption: [#w("Body 2/2").]),
  [
    Each row is a labelled bar with its percent drawn inside it. The *ticks*
    across a bar are the edges of its comfort band. A bar goes *red* when the
    value is out of its band.
  ])

#word-table(see: false,
  table.header([*First page*], [*Ticks and red*]),
  [#w("Full")], [Ticks at #v("FULL_LO_PCT") and #v("FULL_HI_PCT"). Red outside.],
  [#w("Water")], [Ticks at #v("WATER_LO_PCT") and #v("WATER_HI_PCT"). Red outside.],
  [#w("Energy")], [One tick at #v("ENERGY_LO_PCT"). Red under it.],
  [#w("Fun")], [Ticks at #v("FUN_LO_PCT") and #v("FUN_HI_PCT"). Red outside.],
  [#w("Clean")], [One tick at #clean-line(). Red under it. Clean is the
    reverse of dirt.],
)

#word-table(see: false,
  table.header([*Second page*], [*Ticks and red*]),
  [#w("Health")], [One tick at #v("MOOD_HEALTH_FLOOR_PCT"), red under it. A
    second tick is the health ceiling.],
  [#w("Fit")], [One tick at #v("FIT_LO_PCT"). Red under it.],
  [#w("Weight")], [Ticks at #v("WEIGHT_LO_PCT") and #v("WEIGHT_HI_PCT"). Red
    outside.],
  [#w("Booze")], [One tick at #v("BOOZE_HI_PCT"). Red over it.],
  [#w("Mood")], [No ticks. Red under #v("DMG_MOOD_PCT").],
)

Page two also names the pet's lasting conditions under the bars, such as
#w("sore") or #w("injured"). Past two, a count shows after them. The footer
shows the pet's name and its age in days and hours, with a star after the name
if a settings file has changed the balance. A pet without a name shows its
generation instead.

#warn[Strain has no bar. Its only sign is #w("sore") or #w("injured"). Mood
is not a need, so its bar has no band.]

