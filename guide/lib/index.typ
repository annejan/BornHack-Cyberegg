// Chapter 15 helper: the index of on-screen words. Digits live here, never in
// the chapter file.
#import "booklet.typ": *

// The letter an entry is filed under: its first letter, upper case, or "#"
// for a word that starts with a symbol or a digit.
#let _letter(t) = {
  let c = upper(t.clusters().first())
  if c >= "A" and c <= "Z" { c } else { "#" }
}

// Every word the guide marked with `w()`, once, sorted without regard to
// case, with the pages it is marked on (ascending, no repeats), set in two
// columns under sprite dividers.
#let word-index() = context {
  let pages = (:)
  for m in query(<gw>) {
    let pg = m.location().page()
    let have = pages.at(m.value, default: ())
    if pg not in have { have.push(pg) }
    pages.insert(m.value, have)
  }
  let keys = pages.keys().sorted(key: k => (lower(k), k))
  let cur = none
  let n = 0
  columns(2, gutter: 6mm, {
    set text(size: 8pt)
    set par(leading: 0.45em, spacing: 0.45em, justify: false)
    for k in keys {
      let l = _letter(k)
      if l != cur {
        cur = l
        let pet = roster.pets.at(calc.rem(n, roster.pets.len()))
        n += 1
        block(above: 2.2mm, below: 1mm, sticky: true, breakable: false, grid(
          columns: (auto, 1fr), column-gutter: 2mm, align: horizon,
          sprite-fit(pet.prefix, "idle", w: 22pt, h: 16pt),
          text(font: head-font, size: 9pt, fill: ink, l)))
      }
      let pgs = pages.at(k).sorted().map(str).join(", ")
      block(breakable: false, grid(columns: (auto, 1fr, auto), column-gutter: 2pt,
        text(font: screen-font, k),
        box(width: 100%, repeat(text(fill: ink.lighten(50%))[.])),
        text(pgs)))
    }
  })
}
