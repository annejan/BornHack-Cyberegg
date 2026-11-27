// Chapter 4 helpers: the band bars of the nine needs and dose formatting.
// Every edge comes from the numbers dump (`v()` and the `values` table), so a
// retune of the game moves the bars and the prose with it. Digits live here,
// never in the chapter file.
#import "booklet.typ": *
#import "data.typ": *

// The number behind a key, for placing a zone on a bar.
#let num(key, preset: classic) = preset.at("values").at(key)

// Dose amounts are in tenths of a percent: 200 is "20 %", 4 is "0.4 %".
#let tenths(n) = {
  let a = calc.abs(n)
  let frac = calc.rem(a, 10)
  str(calc.quo(a, 10)) + if frac > 0 { "." + str(frac) } else { "" } + " %"
}

// One field of one dose, as a percent. `dose-pct("Pizza", "full")`.
#let dose-pct(item, field, preset: classic) = tenths(dose(item, preset: preset).at(field))

// Zone labels. The second line says where the zone is, strict edges first:
// "under" and "over" exclude the edge, "from" and "to" include it.
#let _two(name, line) = if line == none { name } else [#name #linebreak() #text(size: 6.5pt, line)]
#let under(key, preset: classic) = "under " + v(key, preset: preset)
#let over(key, preset: classic) = "over " + v(key, preset: preset)
#let from(key, preset: classic) = "from " + v(key, preset: preset)
#let upto(key, preset: classic) = "to " + v(key, preset: preset)
#let between(a, b, preset: classic) = v(a, preset: preset) + " to " + v(b, preset: preset)

// zone(from-key-or-number, to-key-or-number, name, kind, line)
#let _n(x, preset) = if type(x) == str { num(x, preset: preset) } else { x }
// A zone under 8 points wide has no room for the unit: "over 95", not "over 95 %".
#let _zone(a, b, name, kind, line, preset) = {
  let (lo, hi) = (_n(a, preset), _n(b, preset))
  (lo, hi, _two(name, if hi - lo < 10 { none } else { line }), kind)
}

// The band bar of one need. Cream is comfort, hatched is a mood pull, red is
// trouble. The marker is for a value, when a page wants to point at one.
#let need-bar(name, preset: classic, value: none) = {
  let z(a, b, label, kind, line) = _zone(a, b, label, kind, line, preset)
  let zones = if name == "full" {
    (
      z(0, "NEED_DANGER_LO_PCT", "starving", "danger", under("NEED_DANGER_LO_PCT", preset: preset)),
      z("NEED_DANGER_LO_PCT", "HUNGRY_PCT", "hungry", "warn", under("HUNGRY_PCT", preset: preset)),
      z("HUNGRY_PCT", "FULL_LO_PCT", "peckish", "warn", under("FULL_LO_PCT", preset: preset)),
      z("FULL_LO_PCT", "FULL_HI_PCT", "comfort", "ok", between("FULL_LO_PCT", "FULL_HI_PCT", preset: preset)),
      z("FULL_HI_PCT", "DMG_NEED_HIGH_PCT", "stuffed", "warn", over("FULL_HI_PCT", preset: preset)),
      z("DMG_NEED_HIGH_PCT", 100, "hurts", "danger", over("DMG_NEED_HIGH_PCT", preset: preset)),
    )
  } else if name == "water" {
    (
      z(0, "NEED_DANGER_LO_PCT", "parched", "danger", under("NEED_DANGER_LO_PCT", preset: preset)),
      z("NEED_DANGER_LO_PCT", "WATER_LO_PCT", "thirsty", "warn", under("WATER_LO_PCT", preset: preset)),
      z("WATER_LO_PCT", "WATER_HI_PCT", "comfort", "ok", between("WATER_LO_PCT", "WATER_HI_PCT", preset: preset)),
      z("WATER_HI_PCT", "DMG_NEED_HIGH_PCT", "waterlogged", "warn", over("WATER_HI_PCT", preset: preset)),
      z("DMG_NEED_HIGH_PCT", 100, "hurts", "danger", over("DMG_NEED_HIGH_PCT", preset: preset)),
    )
  } else if name == "energy" {
    (
      z(0, "EXHAUSTED_PCT", "exhausted", "danger", under("EXHAUSTED_PCT", preset: preset)),
      z("EXHAUSTED_PCT", "ENERGY_LO_PCT", "sleepy", "warn", under("ENERGY_LO_PCT", preset: preset)),
      z("ENERGY_LO_PCT", 100, "comfort: no top edge", "ok", from("ENERGY_LO_PCT", preset: preset)),
    )
  } else if name == "fun" {
    (
      z(0, "FUN_LO_PCT", "bored", "warn", under("FUN_LO_PCT", preset: preset)),
      z("FUN_LO_PCT", "FUN_HI_PCT", "comfort", "ok", between("FUN_LO_PCT", "FUN_HI_PCT", preset: preset)),
      z("FUN_HI_PCT", 100, "wired", "warn", over("FUN_HI_PCT", preset: preset)),
    )
  } else if name == "dirt" {
    (
      z(0, "DIRT_HI_PCT", "comfort", "ok", upto("DIRT_HI_PCT", preset: preset)),
      z("DIRT_HI_PCT", "DIRT_DANGER_PCT", "grubby", "warn", over("DIRT_HI_PCT", preset: preset)),
      z("DIRT_DANGER_PCT", 100, "filthy", "danger", over("DIRT_DANGER_PCT", preset: preset)),
    )
  } else if name == "booze" {
    (
      z(0, "TIPSY_PCT", "sober", "ok", under("TIPSY_PCT", preset: preset)),
      z("TIPSY_PCT", "BOOZE_HI_PCT", "tipsy", "ok", between("TIPSY_PCT", "BOOZE_HI_PCT", preset: preset)),
      z("BOOZE_HI_PCT", "BOOZE_DANGER_PCT", "sloppy", "warn", over("BOOZE_HI_PCT", preset: preset)),
      z("BOOZE_DANGER_PCT", 100, "wasted", "danger", over("BOOZE_DANGER_PCT", preset: preset)),
    )
  } else if name == "weight" {
    (
      z(0, "DMG_WEIGHT_LOW_PCT", "hurts", "danger", under("DMG_WEIGHT_LOW_PCT", preset: preset)),
      z("DMG_WEIGHT_LOW_PCT", "WEIGHT_LO_PCT", "underweight", "warn", under("WEIGHT_LO_PCT", preset: preset)),
      z("WEIGHT_LO_PCT", "WEIGHT_HI_PCT", "comfort", "ok", between("WEIGHT_LO_PCT", "WEIGHT_HI_PCT", preset: preset)),
      z("WEIGHT_HI_PCT", "DMG_WEIGHT_HIGH_PCT", "overweight", "warn", over("WEIGHT_HI_PCT", preset: preset)),
      z("DMG_WEIGHT_HIGH_PCT", 100, "hurts", "danger", over("DMG_WEIGHT_HIGH_PCT", preset: preset)),
    )
  } else if name == "fitness" {
    (
      z(0, "FIT_LO_PCT", "unfit", "warn", under("FIT_LO_PCT", preset: preset)),
      z("FIT_LO_PCT", 100, "comfort: no top edge", "ok", from("FIT_LO_PCT", preset: preset)),
    )
  } else if name == "strain" {
    (
      z(0, "SORE_PCT", "fine", "ok", upto("SORE_PCT", preset: preset)),
      z("SORE_PCT", "STRAIN_INJ_PCT", "sore", "ok", over("SORE_PCT", preset: preset)),
      z("STRAIN_INJ_PCT", 100, "injury", "danger", over("STRAIN_INJ_PCT", preset: preset)),
    )
  } else {
    panic("no such need: " + name)
  }
  band-bar(zones, value: value)
}

// The key to every bar, set once on the chapter's first page.
#let _swatch(fill) = box(baseline: 20%, width: 9pt, height: 7pt, stroke: 1pt + ink, fill: fill)
#let bar-key() = {
  let hatch = tiling(size: (4pt, 4pt), place(line(start: (0pt, 4pt), end: (4pt, 0pt), stroke: 0.8pt + ink)))
  grid(columns: 3, column-gutter: 10pt, align: horizon,
    [#_swatch(cream) comfort],
    [#_swatch(hatch) a mood pull],
    [#_swatch(rgb("#c0392b")) trouble])
}

// One row of the Classic against Casual table. `how`: "band" (two edges),
// "from" (a floor), "upto" (a ceiling) or "over" (a line the value must pass).
#let edge-row(label, shown, how, lo, hi: none) = {
  let cell(preset) = if how == "band" { between(lo, hi, preset: preset) }
    else if how == "from" { from(lo, preset: preset) }
    else if how == "upto" { upto(lo, preset: preset) }
    else { over(lo, preset: preset) }
  (label, shown, cell(classic), cell(casual))
}

// A dose field, or `alt` when that dose leaves the field alone.
#let dose-pct-or(item, field, alt, preset: classic) = {
  let d = dose(item, preset: preset)
  if field in d { tenths(d.at(field)) } else { alt }
}

// Where the Clean bar's tick sits: Clean is 100 minus dirt.
#let clean-line(preset: classic) = str(100 - num("DIRT_HI_PCT", preset: preset)) + " %"
#let clean-at(dirt, preset: classic) = str(100 - num(dirt, preset: preset)) + " %"

// A table with a header row, for the dose tables.
#let dose-table(cols, ..cells) = table(columns: cols, stroke: 0.5pt + ink, inset: 3.5pt, align: left, ..cells)

// The Dirt row of the glance table, seen from the Clean side: Clean is 100 minus dirt.
#let clean-row() = {
  let cell(preset) = "from " + clean-at("DIRT_HI_PCT", preset: preset)
  ("Dirt", [#w("Clean"), reversed], cell(classic), cell(casual))
}
