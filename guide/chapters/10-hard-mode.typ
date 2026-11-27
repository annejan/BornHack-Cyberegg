#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/social.typ": *
#import "../lib/us.typ": *
#chapter("ch-hard")[Hard mode (US)]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

#us-row()

Welcome to the land of the free, the brave and the large fries. Hard mode
is money mode with a bigger bill. This chapter says what changes. It
describes what the badge does today, and nothing more.

== How to pick it

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    You pick the mode when you pick the egg (@ch-start). After the kind, the
    badge asks about money. The third row is #w("Hard mode (US)"). Press Fire
    on it and the egg hatches in the dearer mode. Money is on, so the balance
    shows and every priced row shows its price (@ch-money).

    The mode is fixed for that pet's life. A new egg lets you pick again.
    The egg starts with the same purse as in the other money mode:
    #v("START_HAX").
  ],
  screen("select-hard", width: shot-m, caption: [The third row.]))

== What costs more

Only a few prices change. Everything else is the same as in money mode, and
every row that is free there is free here.

- *Healthy food.* #w("Salad") and #w("Apple") cost #price("Salad", hard: true)
  each, up from #price("Salad"). The healthy food was already the dear food.
- *Medicine.* #w("Insulin") and #w("Ozempic") cost #v("MEDICATION_HARD_MULT")
  times the usual price. #w("Rehab") costs #v("REHAB_HARD_MULT") times.
- *Unchanged.* #w("Pizza"), #w("Cake") and all the drinks keep their prices.
  So does #w("Aspirine").

A price is still paid when the action starts, and a refused row still costs
nothing. The #w("can't afford") rule is the same too (@ch-money).

#block[
#set text(size: 8pt, hyphenate: false)
#tbl((1fr, auto, auto),
  table.header([*Row*], [*Money mode*], [*Hard mode*]),
  ..prow(w("Salad"), "Salad", hard: true),
  ..prow(w("Apple"), "Apple", hard: true),
  ..prow(w("Hamburger"), "Frikandel spec", hard: true),
  ..prow(w("Pizza"), "Pizza", hard: true),
  ..prow(w("Cake"), "Cake", hard: true),
  ..prow(w("Water"), "Water", hard: true),
  ..prow(w("Coffee"), "Coffee", hard: true),
  ..prow(w("Beer"), "Beer", hard: true),
  ..prow(w("Wine"), "Wine", hard: true),
  ..prow(w("Whiskey"), "Whiskey", hard: true),
  ..prow(w("Aspirine"), "Heal", hard: true),
  ..prow(w("Insulin"), "Medicate", hard: true),
  ..prow(w("Ozempic"), "Ozempic", hard: true),
  ..prow(w("Rehab"), "Rehab", hard: true),
)
]

#warn[Rehab is the big one. Keep the pet away from the bottle, and watch
the alarms (@ch-conditions). It is cheaper to avoid the trip.]

== The Hamburger

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    In this mode the #w("Frikandel speciaal") is drawn as a #w("Hamburger").
    It is the same food. The new name shows in two places: in the #w("Feed")
    list row, and in the title bar when that row is selected. Nothing else is
    new.
    It keeps the Frikandel's price and the Frikandel's effects, so what
    @ch-care says about the Frikandel holds for the Hamburger too.
  ],
  screen("menu-feed-hard", width: shot-m, caption: [Salad and apple cost more. The hamburger does not.]))

#joke[The Hamburger is a Frikandel in a flag. In the story, it is bigger than
the pet's head. The badge does not mind, and neither does the pet.]

== The fine print

The satire here is about the system, never about the people in it. Real
paperwork is long. The badge is short.

#joke[A real clinic sends forms, then a letter, then another letter. This
badge takes the price when you press Fire. Nobody puts you on hold, and
there is no hold music.]

#joke[Pets in this mode are not insured. The badge does not ask for a card,
a form or a second opinion. It asks only that you can pay.]

#joke[A bald eagle watches over this chapter. It does not feed the pet. It
does not pay for rehab. It just looks proud, and that is all it does.]

Plan to have a full purse before the first bad week.

#block(height: 8mm)
#us-row-end()
