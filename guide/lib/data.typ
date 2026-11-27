// The numbers dump (guide/build/data/*.json, made by `guide data`).
// A missing key fails the compile: always `.at(key)`, never `.at(key, default: ..)`.
#let classic = json("/guide/build/data/classic.json")
#let casual = json("/guide/build/data/casual.json")

#let _num(n) = str(n)

// A length of time in minutes: "45 min", "6 h", "6 h 30 min", "3 days".
#let fmt-min(m) = {
  if m >= 1440 and calc.rem(m, 1440) == 0 {
    let d = calc.quo(m, 1440)
    _num(d) + if d == 1 { " day" } else { " days" }
  } else if m >= 60 {
    let h = calc.quo(m, 60)
    let r = calc.rem(m, 60)
    _num(h) + " h" + if r > 0 { " " + _num(r) + " min" } else { "" }
  } else {
    _num(m) + " min"
  }
}

// A length of time in seconds, down to the second.
#let fmt-secs(s) = {
  if s < 60 {
    _num(s) + " s"
  } else {
    let r = calc.rem(s, 60)
    fmt-min(calc.quo(s, 60)) + if r > 0 { " " + _num(r) + " s" } else { "" }
  }
}

#let _fmt(key, n, unit) = {
  if unit == "pct" { _num(n) + " %" }
  else if unit == "min" { fmt-min(n) }
  else if unit == "ticks" { fmt-secs(n * 10) }
  else if unit == "secs" { fmt-secs(n) }
  else if unit == "count" { _num(n) }
  else if unit == "hax" { _num(n) + " HAX" }
  else { panic("use a derived number, not " + key) }
}

// A number from the data, formatted by its unit. Raw units are refused.
#let v(key, preset: classic) = _fmt(key, preset.at("values").at(key), preset.at("units").at(key))

// The bare number, for the appendix table only.
#let raw-v(key, preset: classic) = _num(preset.at("values").at(key))

// Do Classic and Casual disagree on this key?
#let differs(key) = classic.at("values").at(key) != casual.at("values").at(key)

#let price(item, hard: false, preset: classic) = {
  let p = preset.at("prices").at(if hard { "hard" } else { "normal" }).at(item)
  if p == none { "free" } else { _num(p) + " HAX" }
}

#let until(word, preset: classic) = {
  let m = preset.at("until").at(word)
  if m == none { panic("no time for " + word) }
  fmt-min(m)
}

#let night(preset: classic) = {
  let m = preset.at("night_min")
  if m == none { panic("no night length") }
  fmt-min(m)
}

#let dose(item, preset: classic) = preset.at("doses").at(item)
