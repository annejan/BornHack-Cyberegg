// Chapter 8, 9, 11 and 12 helpers: price rows, the off-time timeline, the
// battle picture and text the badge draws that no word list holds. Digits
// live here, never in the chapter files.
#import "booklet.typ": *
#import "data.typ": *
#import "needs.typ": *
#import "care.typ": *

// A price table row: the item as shown, its price in money mode, and what it
// costs without money. Every price comes from the numbers dump.
#let prow(label, item, hard: false) = if hard {
  (label, price(item), price(item, hard: true))
} else {
  (label, price(item), [free])
}

// One box of a flow row.
#let _step(title, sub, fill: white) = box(
  width: 100%,
  stroke: 1.5pt + ink,
  fill: fill,
  inset: 4pt,
  align(center, {
    text(font: head-font, size: 5pt, title)
    linebreak()
    text(size: 7.5pt, sub)
  }),
)
#let _arrow() = box(width: 100%, align(center, text(size: 10pt, sym.arrow.r)))

// What happens to the time the badge spends off.
#let offtime-diagram() = diagram-box(
  grid(
    columns: (1fr, 0.25fr, 1fr, 0.25fr, 1fr),
    align: horizon,
    column-gutter: 1pt,
    _step("BADGE OFF", [No clock runs. No time passes.], fill: rgb("#f1b8a8")),
    _arrow(),
    _step("BADGE ON", [Clock unknown. The pet runs on real time.], fill: cream),
    _arrow(),
    _step(
      "CLOCK SET",
      [One catch-up, at most #v("CATCH_UP_MAX_MIN").],
      fill: rgb("#e4efc9"),
    ),
  ),
)

// An HP bar for the battle picture: `share` percent of the bar is full.
#let _hp(share) = box(
  width: 100%,
  height: 7pt,
  stroke: 1.5pt + ink,
  fill: white,
  align(left, rect(width: share * 1%, height: 100%, fill: ink, stroke: none)),
)

// Two pets facing each other, with their HP bars.
#let versus() = diagram-box(
  grid(
    columns: (1fr, auto, 1fr),
    align: horizon + center,
    column-gutter: 4mm,
    stack(
      spacing: 3pt,
      sprite-fit("hero", "happy", w: 46pt, h: 46pt),
      _hp(70),
      text(size: 8pt)[You],
    ),
    text(font: head-font, size: 9pt)[VS],
    stack(
      spacing: 3pt,
      sprite-fit(roster.pets.at(1).prefix, "idle", w: 46pt, h: 46pt),
      _hp(35),
      text(size: 8pt)[Friend],
    ),
  ),
)

// The six kinds in a row, each at one common scale.
#let realm-row() = grid(
  columns: roster.pets.map(_ => 1fr),
  column-gutter: 2mm,
  align: bottom + center,
  ..roster.pets.map(p => sprite-fit(p.prefix, "happy", w: 34pt, h: 40pt)),
)

// The path from Gone to a new egg.
#let gone-to-egg() = grid(
  columns: (1fr, 0.2fr, 1fr, 0.2fr, 1fr),
  align: horizon + center,
  column-gutter: 1pt,
  screen("gone", width: shot-s, caption: [Gone.]),
  _arrow(),
  screen("select-pet", width: shot-s, caption: [Pick a kind.]),
  _arrow(),
  screen("egg", width: shot-s, caption: [A new egg.]),
)

// The price table columns: the row, the price with money, the price without.
#let price-cols = (1fr, auto, auto)

// The three station kinds, small.
#let station-row() = grid(
  columns: (1fr, 1fr, 1fr),
  align: bottom + center,
  ..(("feeding", [Food.]), ("healing", [Medic.]), ("sleeping", [Rest.])).map(i => stack(
    spacing: gap-s,
    sprite-fit("hero", i.at(0), w: 36pt, h: 36pt),
    text(size: 8pt, i.at(1)),
  )),
)
