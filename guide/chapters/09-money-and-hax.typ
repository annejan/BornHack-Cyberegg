#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/social.typ": *
#chapter("ch-money")[Money and HAX]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

HAX is the badge's money. It is the only money in the game. This chapter
says what a new pet starts with, what costs HAX, how to earn more, and what
happens when the purse is empty. It describes the two ordinary modes. The
dearer mode has its own chapter: @ch-hard.

== Two ways to play

You pick the mode when you pick the egg (@ch-start). It is fixed for that
pet's life, and a new egg lets you pick again. The #w("Mode:") row in the
#w("Bornagotchi") menu is something else: it changes Classic or Casual
(@ch-hackers), not the money.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    - #w("With money"): the egg starts with #v("START_HAX"). The balance
      shows on the pet screen (@ch-reading). Food, drink and medicine cost
      HAX. Some actions pay it.
    - #w("Without money"): no balance, no prices and no rewards. Every row in
      every menu is free.
  ],
  hero-pic("happy"))

A new egg always starts with #v("START_HAX"). The balance belongs to the pet,
so it goes with the pet. It never carries over.

*Demo mode keeps HAX running.* A pet in demo mode (@ch-care) shows the balance
and the prices like any other, and pays for its own meals and medicine. It earns
the money back itself: it plays its mini-games, goes to work (#w("Only pets"))
when the balance is under #v("DEMO_HAX_TARGET") HAX, and takes a battle prize
like any pet. Above that line the balance slowly drains, one HAX every
#v("DEMO_HAX_DRIFT_MIN") minutes, until it is back at the target, so a demo pet
stays near #v("DEMO_HAX_TARGET") HAX. Turning demo off keeps whatever balance
there is.

== The price list

A price is paid when the action starts, and only if the badge accepts the
action. A refused row costs nothing. These are the prices in money mode.
Without money, every row is free.

#block[
#set text(size: 8pt, hyphenate: false)
#grid(columns: (1fr, 1fr), column-gutter: 4mm, align: top,
  tbl(price-cols,
    table.header([*Row*], [*With money*], [*Without*]),
    ..prow(w("Salad"), "Salad"),
    ..prow(w("Apple"), "Apple"),
    ..prow(w("Frikandel spec"), "Frikandel spec"),
    ..prow(w("Pizza"), "Pizza"),
    ..prow(w("Cake"), "Cake"),
    ..prow(w("Water"), "Water"),
    ..prow(w("Coffee"), "Coffee"),
    ..prow(w("Beer"), "Beer"),
    ..prow(w("Wine"), "Wine"),
    ..prow(w("Whiskey"), "Whiskey"),
  ),
  tbl(price-cols,
    table.header([*Row*], [*With money*], [*Without*]),
    ..prow(w("Aspirine"), "Heal"),
    ..prow(w("Insulin"), "Medicate"),
    ..prow(w("Ozempic"), "Ozempic"),
    ..prow(w("Rehab"), "Rehab"),
    ..prow(w("Play now"), "Play"),
    ..prow(w("Exercise now"), "Exercise"),
    ..prow(w("Wash"), "Wash"),
    ..prow(w("Relax"), "Relax"),
    ..(w("Only pets"), price("OnlyPets"), [no row]),
  ))
]

Everything else is free in both modes: #w("Sleep"), #w("Wake up"), the
mini-games, #w("Battle"), the Stats menu and the Hibernate menu. In money
mode a priced row shows its price, like #w("<price>H"), on the right edge.

#tip[Salad and apple cost more than pizza and cake. The healthy food is the
dear food. @ch-care says what each one does.]

== Earning HAX

Three things pay, and only in money mode. A refused row pays nothing, and
so does a mini-game you leave before it ends.

#tbl((1fr, auto, 1fr),
  table.header([*What*], [*Pays*], [*It costs*]),
  [#w("Only pets") (from the Play menu)], v("ONLYPETS_HOBBY_REWARD"),
  [Energy #sg("OnlyPets", "energy"), mood #sg("OnlyPets", "mood"). Fun
   #sg("OnlyPets", "fun"). Takes #v("PLAY_DURATION").],
  [A finished mini-game: win, draw or loss],
  [#v("MINIGAME_HAX_REWARD"), #v("MINIGAME_HAX_DRAW"), #v("MINIGAME_HAX_LOSS")],
  [Energy -#v("DOSE_MINI_ENERGY_PCT"). That game rests for
   #v("MINIGAME_COOLDOWN"). The other four stay open.],
  [A Battle win (first of the day, then later)], [#v("BATTLE_PRIZE_FIRST_HAX"), then #v("BATTLE_PRIZE_LATER_HAX")],
  [Rests for #v("BATTLE_COOLDOWN"). See @ch-friends.],
)

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    #w("Only pets") is the pet's job. It is never held back by the price, so
    a broke pet can work its way out. It is refused when the pet is
    #w("asleep"), #w("injured") or #w("too tired"). The reward is a flat
    amount: it does not depend on the pet's mood.

    A mini-game pays by its result, win, draw or loss, and the same whatever
    the difficulty. A Battle pays only when you win. @ch-friends tells what the other badge gets.
  ],
  hero-pic("exercising"))

#sidebar("CASUAL", sprite-fit("hero", "happy", w: 40pt, h: 40pt), rgb("#e4efc9"))[
  Casual shortens a mini-game's rest to #v("MINIGAME_COOLDOWN", preset: casual)
  and moves the tired line for #w("Only pets") to
  #v("ENERGY_LO_PCT", preset: casual). The prices and rewards are the same.
]

#joke[Only pets pays the same whether the pet likes the work or not. This is
a mechanic the badge actually has. It is not a metaphor for anything.]

== Can't afford

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    A row you cannot pay for is grey, and the cursor skips it. If a buy is
    refused, the title bar says #w("can't afford"). Nothing is charged, and
    the pet is not harmed. The balance never goes below zero. The pet's #w("Hint") row never suggests
    a row you cannot afford.

    *Broke?* The free rows still work. In order of effort:
    - Finish a mini-game. It needs no money and little energy.
    - Send the pet to #w("Only pets"), if it has the energy.
    - Win a Battle against a friend.
    - Wait. Sleep, Relax, Wash and Exercise are free, so the pet is never
      stuck with nothing to do.
  ],
  screen("menu-feed", width: shot-m, caption: [A priced menu.]))

== Playing without money

All the rules are the same. Only the money side is gone.

- No balance on the pet screen. The spot stays empty.
- No prices on any row. #w("can't afford") never shows.
- The Play menu has no #w("Only pets") row.
- Games pay nothing. A finished mini-game still gives its fun. A won Battle still
  counts in the record.

Pick this mode for a relaxed week. Pick the other to feel what a pet costs.
