// Chapter 5 helpers: signed dose cells, price cells and the mini-game fun
// numbers. Every number comes from the numbers dump; digits live here, never
// in the chapter file.
#import "booklet.typ": *
#import "data.typ": *
#import "needs.typ": *

// One field of one dose, with its direction: "+25 %" or "-3 %".
#let sg(item, field, preset: classic) = {
  let n = dose(item, preset: preset).at(field)
  (if n < 0 { "-" } else { "+" }) + tenths(n)
}

// A price cell: the normal price, and the Hard (US) price when it differs.
#let pr(item) = {
  let n = price(item)
  let h = price(item, hard: true)
  if n == h { n } else { [#n #linebreak() #text(size: 7pt)[Hard: #h]] }
}

// Fun from a finished mini-game of grade 0 (loss), 1 (draw) or 2 (win).
#let mini-fun(grade, preset: classic) = str(
  num("DOSE_MINI_FUN_BASE_PCT", preset: preset)
    + num("DOSE_MINI_FUN_GRADE_PCT", preset: preset) * grade) + " %"

// Fun of a tired pet's mini-game or Play now: the usual dose doubled.
#let doubled(n) = str(n * 2) + " %"
#let play-fun-tired() = doubled(num("DOSE_PLAY_FUN_PCT"))

// A clean level the Wash row needs the pet to be under: Clean over 100 minus dirt.
#let wash-clean() = clean-at("WASH_REFUSE_PCT")

// The hero at the gym, bigger than a caption sprite.
#let gym-pic() = sprite-fit("hero", "exercising", w: 90pt, h: 90pt)

// A dose table that never splits across a page.
#let tbl(cols, ..cells) = block(breakable: false, dose-table(cols, ..cells))

// The frozen hero, small, beside the Hibernate text.
#let ice-pic() = sprite-fit("hero", "hibernating", w: 50pt, h: 50pt)

// Like `sg`, but a dose that leaves the field alone (absent or zero) reads "none".
#let sgn(item, field, preset: classic) = {
  let d = dose(item, preset: preset)
  if field in d and d.at(field) != 0 { sg(item, field, preset: preset) } else { "none" }
}
