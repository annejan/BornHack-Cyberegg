// Chapter 6 and 7 helpers: the mood diagram, health rates as sweep times and
// the raw-to-percent conversion. Every number comes from the numbers dump;
// digits live here, never in the chapter files.
#import "booklet.typ": *
#import "data.typ": *
#import "needs.typ": *

// The level scale of the engine: a full bar is this many raw steps, and one
// percent is this many (see `needs::MAX` and `needs::P`).
#let level-max = 65535
#let level-pct = 655.35

// How long a rate (raw steps per minute) takes to sweep a whole bar.
#let sweep(key, preset: classic) = fmt-min(calc.quo(level-max, num(key, preset: preset)))

// A raw amount as percent points of a bar: 1965 raw is "3 %".
#let raw-pct(key, preset: classic) = {
  let tenth = calc.round(num(key, preset: preset) / level-pct * 10)
  let whole = calc.quo(tenth, 10)
  let frac = calc.rem(tenth, 10)
  str(whole) + if frac > 0 { "." + str(frac) } else { "" } + " %"
}

// One box of the flow diagram.
#let _node(title, sub, fill: white) = box(width: 100%, stroke: 1.5pt + ink, fill: fill, inset: 4pt,
  align(center, {
    text(font: head-font, size: 5pt, title)
    linebreak()
    text(size: 7.5pt, sub)
  }))

#let _arrow(label) = box(width: 100%, align(center, stack(spacing: 2pt,
  text(size: 6.5pt, label), text(size: 10pt, sym.arrow.r))))

// Needs, mood target, mood and health, and the arrows between them, with the
// shortcuts under the row. The numbers are the code's.
#let mood-flow() = diagram-box({
  grid(columns: (1.1fr, 1fr, 1.1fr, 1fr, 1.1fr, 1fr, 1.1fr), align: horizon, column-gutter: 1pt,
    _node("NEEDS", [nine bars], fill: rgb("#e4efc9")),
    _arrow([off its band]),
    _node("TARGET", [full, less the pulls], fill: rgb("#f6dd9a")),
    _arrow([one step a minute]),
    _node("MOOD", [the pet's face], fill: rgb("#f6dd9a")),
    _arrow([heals or hurts]),
    _node("HEALTH", [how long it stays], fill: rgb("#f1b8a8")))
  std.v(3pt)
  set text(size: 8pt)
  align(left, [
    #sym.arrow.r Mood #sym.gt.eq #v("REC_GOOD_MOOD_PCT"): health heals. Mood under
    #v("DMG_MOOD_PCT"): health drains. \
    #sym.arrow.r Needs past a danger line drain health directly. \
    #sym.arrow.r Fitness sets the *ceiling* health cannot pass. \
    #sym.arrow.r Health under #v("MOOD_HEALTH_FLOOR_PCT") pulls the target down. \
    #sym.arrow.r Conditions (hangover, injury and more) pull the target down. \
    #sym.arrow.r Health at zero starts the leaving clock.
  ])
})

// The health ceiling as four steps of fitness.
#let ceiling-bar() = band-bar((
  (0, num("CEILING_FIT_STEP1_PCT"), [fit under #v("CEILING_FIT_STEP1_PCT") #linebreak() ceiling #v("CEILING_STEP0_PCT")], "danger"),
  (num("CEILING_FIT_STEP1_PCT"), num("CEILING_FIT_STEP2_PCT"), [from #v("CEILING_FIT_STEP1_PCT") #linebreak() ceiling #v("CEILING_STEP1_PCT")], "warn"),
  (num("CEILING_FIT_STEP2_PCT"), num("CEILING_FIT_STEP3_PCT"), [from #v("CEILING_FIT_STEP2_PCT") #linebreak() ceiling #v("CEILING_STEP2_PCT")], "warn"),
  (num("CEILING_FIT_STEP3_PCT"), 100, [from #v("CEILING_FIT_STEP3_PCT") #linebreak() ceiling #v("CEILING_STEP3_PCT")], "ok"),
))

// One row of the damage table: what hurts, and how long that alone takes to
// empty a full health bar, in Classic and in Casual.
#let dmg-row(what, line, key) = (what, line, sweep(key), sweep(key, preset: casual))

// The two diabetes weight lines on one weight bar.
#let diabetes-bar() = band-bar((
  (0, num("DIABETES_WEIGHT_LO_PCT"), [pays back #linebreak() under #v("DIABETES_WEIGHT_LO_PCT")], "ok"),
  (num("DIABETES_WEIGHT_LO_PCT"), num("DIABETES_WEIGHT_HI_PCT"), [frozen], "warn"),
  (num("DIABETES_WEIGHT_HI_PCT"), 100, [counts up #linebreak() over #v("DIABETES_WEIGHT_HI_PCT")], "danger"),
))

// The two booze lines on one booze bar.
#let dependence-bar() = band-bar((
  (0, num("BOOZE_CLEAR_PCT"), [pays back #linebreak() under #v("BOOZE_CLEAR_PCT")], "ok"),
  (num("BOOZE_CLEAR_PCT"), num("BOOZE_HI_PCT"), [frozen], "warn"),
  (num("BOOZE_HI_PCT"), 100, [counts up #linebreak() over #v("BOOZE_HI_PCT")], "danger"),
))

// The hero sick, a bit bigger, for a condition's lead.
#let sick-pic() = sprite-fit("hero", "sick", w: 60pt, h: 60pt)

// The sick hero over the sleeping hero.
#let sick-sleep-pics() = stack(spacing: gap-s, sick-pic(), sprite-fit("hero", "sleeping", w: 60pt, h: 60pt))

// The cause-toast table: toast, why it fires, what to do.
#let toast-table(..rows) = block(table(
  columns: (auto, 1fr, 1fr), stroke: 0.5pt + ink, inset: 2.5pt, align: left + top,
  table.header([*Toast*], [*Why it fires*], [*What to do*]), ..rows))

// Table shapes for chapter 6.
#let pull-table(..cells) = dose-table((auto, 1fr), ..cells)
#let flat-table(..cells) = dose-table((auto, auto), ..cells)
#let dmg-table(..cells) = block(breakable: false, dose-table((auto, 1fr, auto, auto), ..cells))
