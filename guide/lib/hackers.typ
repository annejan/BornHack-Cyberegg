// Chapter 13 and 14 helpers: the secret code, a stamp and the balance table.
// Digits live here, never in the chapter files.
#import "booklet.typ": *
#import "data.typ": *

// One small keycap of the secret code. `label` is an arrow or a word.
#let _cap(label, fire: false) = box(
  width: if fire { 15mm } else { 9mm }, height: 9mm, radius: if fire { 4.5mm } else { 1.5mm },
  stroke: 2pt + cream, fill: if fire { rgb("#f6dd9a") } else { ink },
  align(center + horizon, text(font: head-font, size: if fire { 5.5pt } else { 9pt },
    fill: if fire { ink } else { cream }, label)))

// The nine presses, as arrows, on a dark panel (the page's visual trick).
#let secret-code() = {
  pic()
  block(width: 100%, breakable: false, fill: ink, inset: 8pt, stroke: 2.5pt + ink, align(center, {
    text(font: head-font, size: 6pt, fill: cream)[IDLE ON THE GAME SCREEN]
    block(height: 3mm)
    grid(columns: 9, column-gutter: 1.2mm, align: horizon + center,
      _cap(sym.arrow.t), _cap(sym.arrow.t), _cap(sym.arrow.b), _cap(sym.arrow.b),
      _cap(sym.arrow.l), _cap(sym.arrow.r), _cap(sym.arrow.l), _cap(sym.arrow.r),
      _cap("FIRE", fire: true))
  }))
}

// A tilted red stamp.
#let stamp(label) = rotate(-8deg, box(stroke: 2.5pt + rgb("#c0392b"), inset: (x: 6pt, y: 3pt),
  text(font: head-font, size: 10pt, fill: rgb("#c0392b"), label)))

// The unit column of the balance table.
#let unit-label(key) = {
  let u = classic.at("units").at(key)
  if u == "pct" { "%" } else if u == "hax" { "HAX" } else if u == "secs" { "s" } else { u }
}

// One table row: Casual in bold where it differs from Classic.
#let cfg-row(key) = (
  raw(key),
  unit-label(key),
  raw-v(key),
  if differs(key) { strong(raw-v(key, preset: casual)) } else { raw-v(key, preset: casual) },
)

// Every key `BPETS_V3.CFG` takes: unit, Classic and Casual.
#let cfg-table() = {
  set text(size: 7.5pt)
  set par(justify: false)
  table(
    columns: (1fr, auto, auto, auto), stroke: 0.4pt + ink, inset: (x: 3pt, y: 1.8pt),
    align: (left, left, right, right), fill: (_, y) => if y > 0 and calc.even(y) { white.transparentize(60%) },
    table.header(repeat: true, [*Key*], [*Unit*], [*Classic*], [*Casual*]),
    ..classic.at("cfg_keys").map(cfg-row).flatten(),
  )
}

// The two example files of chapter 14, shown as the badge reads them.
#let cfg-example() = block(breakable: false, raw(block: true,
  "# FULL_LO_PCT=" + raw-v("FULL_LO_PCT") + "   # Casual " + raw-v("FULL_LO_PCT", preset: casual)
    + "\nMOOD_STEP=200"))
#let pets-example() = block(breakable: false, raw(block: true,
  "# PREFIX=NAME\n" + "0=Bartholomeus\n" + "5=Panda"))
