// Booklet look: A5 portrait, cream page, dark ink, chunky Amiga-style borders.
#let cream = rgb("#f3ead3")
#let ink = rgb("#2a2118")
#let head-font = "Press Start 2P"
#let body-font = "Libertinus Serif"
#let screen-font = "DejaVu Sans Mono"

// Chunky box used for sidebars and frames.
#let chunky(body, fill: none, inset: 8pt) = block(
  width: 100%,
  inset: inset,
  stroke: 2.5pt + ink,
  radius: 0pt,
  fill: fill,
  body,
)

// An on-screen word, as the badge draws it: monospace on a white chip. Every
// use is also an index mark (`<gw>`): build_guide.py checks the marks against
// guide/build/data/words.json, both ways.
#let w(t) = {
  box(
    fill: white,
    stroke: 0.5pt + ink,
    inset: (x: 2pt, y: 0pt),
    outset: (y: 2pt),
    text(font: screen-font, size: 0.9em, t),
  )
  [#metadata(t) <gw>]
}

// The sprite roster (guide/build/sprites/roster.json, made by `guide sprites`).
// A missing file or key fails the compile.
#let roster = json("/guide/build/sprites/roster.json")

// Marks the page as having a picture. build_guide.py queries `<pic>` and fails
// any page from 2 up to the back cover that lacks one.
#let pic() = context [#metadata(here().page()) <pic>]

// The guide's walking example (the first pet kind of the roster).
#let hero = roster.hero.name

// A simulator screen in a bezel frame. `name` is a shot of `guide shots`.
#let screen(name, width: 46mm, caption: none) = {
  pic()
  let img = image("/guide/build/screens/" + name + ".png", width: width, scaling: "pixelated")
  let framed = box(
    stroke: 2.5pt + ink,
    inset: 3pt,
    fill: rgb("#6d6a63"),
    box(stroke: 1pt + ink, img),
  )
  if caption == none {
    framed
  } else {
    box(width: width + 12pt, align(center, stack(spacing: 3pt, framed, text(size: 8pt, caption))))
  }
}

// One frame of a pet animation, at a whole-number scale. The PNGs are cropped
// to the pet, one PNG pixel per badge pixel. `k` badge pixels become k
// `sprite-px` units of paper, so every pixel is the same size. `pet` is a
// prefix as in roster.json ("00", "05", ...) or "hero". No free mm or % sizes.
#let sprite-px = 0.5 // pt of paper per badge pixel, per unit of k
#let sprite-path(pet, anim, frame) = {
  let prefix = if pet == "hero" { roster.hero.prefix } else { pet }
  "/guide/build/sprites/" + prefix + "-" + anim + "-" + str(frame) + ".png"
}
#let sprite(pet, anim, frame: 0, k: 2) = {
  pic()
  let path = sprite-path(pet, anim, frame)
  context {
    let n = measure(image(path))
    image(path, width: n.width * k * sprite-px, scaling: "pixelated")
  }
}
// The biggest whole-number scale that keeps the sprite inside `w` x `h`.
#let sprite-fit(pet, anim, frame: 0, w: 60pt, h: 60pt) = {
  let path = sprite-path(pet, anim, frame)
  context {
    let n = measure(image(path))
    let k = calc.max(1, calc.floor(calc.min(w / (n.width * sprite-px), h / (n.height * sprite-px))))
    sprite(pet, anim, frame: frame, k: k)
  }
}

// Several animations of one pet at one common whole-number scale (the biggest
// that keeps all of them inside `w` x `h`). Returns an array of sprites.
#let sprite-set(pet, anims, w: 60pt, h: 60pt) = anims.map(a => {
  let path = sprite-path(pet, a, 0)
  context {
    let ks = anims.map(b => {
      let n = measure(image(sprite-path(pet, b, 0)))
      calc.floor(calc.min(w / (n.width * sprite-px), h / (n.height * sprite-px)))
    })
    sprite(pet, a, k: calc.max(1, calc.min(..ks)))
  }
})

// Pixel heart drawn from rectangles (no sprite needed).
#let pixel-heart(size: 6pt, color: rgb("#c0392b")) = {
  let rows = (
    "01100110",
    "11111111",
    "11111111",
    "11111111",
    "01111110",
    "00111100",
    "00011000",
  )
  box(width: 8 * size, height: 7 * size, {
    for (y, row) in rows.enumerate() {
      for (x, ch) in row.clusters().enumerate() {
        if ch == "1" {
          place(top + left, dx: x * size, dy: y * size, rect(width: size, height: size, fill: color, stroke: none))
        }
      }
    }
  })
}

// A US flag from square "pixels": 13 stripes, a canton of dots.
#let pixel-flag(width) = {
  pic()
  let cols = 20
  let px = width / cols
  let red = rgb("#b22234")
  let blue = rgb("#3c3b6e")
  box(width: width, height: 13 * px * 0.9, stroke: 1.5pt + ink, {
    for r in range(13) {
      let y = r * px * 0.9
      for c in range(cols) {
        let in-canton = r < 7 and c < 8
        let fill = if in-canton { blue } else if calc.even(r) { red } else { white }
        place(top + left, dx: c * px, dy: y, rect(width: px, height: px * 0.9, fill: fill, stroke: none))
        if in-canton and calc.odd(r + c) and r > 0 and r < 6 and c > 0 and c < 7 {
          place(top + left, dx: c * px + px * 0.3, dy: y + px * 0.25,
            rect(width: px * 0.4, height: px * 0.4, fill: white, stroke: none))
        }
      }
    }
  })
}

// A frame for a hand-drawn diagram.
#let diagram-box(body) = {
  pic()
  chunky(align(center, body), fill: white.transparentize(70%))
}

// A 0 to 100 bar. `zones`: (from, to, label, kind), kind "ok" (cream),
// "warn" (hatched) or "danger" (red). `value`: optional marker.
#let band-bar(zones, value: none, width: 100%) = {
  pic()
  let hatch = tiling(size: (4pt, 4pt), {
    place(line(start: (0pt, 4pt), end: (4pt, 0pt), stroke: 0.8pt + ink))
  })
  let fills = (ok: cream, warn: hatch, danger: rgb("#c0392b"))
  block(width: width, breakable: false, layout(size => {
    let bar-w = size.width
    let h = 9pt
    box(width: bar-w, height: h + 22pt, {
      for z in zones {
        let x0 = bar-w * z.at(0) / 100
        let zw = bar-w * (z.at(1) - z.at(0)) / 100
        place(top + left, dx: x0, rect(width: zw, height: h, fill: fills.at(z.at(3)), stroke: 1.5pt + ink))
        place(top + left, dx: x0, dy: h + 3pt,
          box(width: zw, align(center, text(size: 7pt, z.at(2)))))
      }
      if value != none {
        let mx = bar-w * value / 100
        place(top + left, dx: mx - 3pt, dy: -3pt,
          polygon(fill: ink, (0pt, 0pt), (6pt, 0pt), (3pt, 6pt)))
        place(top + left, dx: mx - 0.75pt, dy: 0pt, rect(width: 1.5pt, height: h, fill: ink, stroke: none))
      }
    })
  }))
}

// Sidebar boxes: a chunky border, a small pixel icon, a title.
#let sidebar(title, icon, fill, body) = block(
  width: 100%,
  breakable: false,
  chunky(
    grid(columns: (14mm, 1fr), column-gutter: 8pt, align: horizon,
      box(width: 14mm, icon),
      {
        set par(justify: false)
        text(font: head-font, size: 6.5pt, title)
        parbreak()
        body
      }),
    fill: fill,
  ),
)
#let tip(body) = sidebar("TIP", sprite-fit("hero", "happy", w: 40pt, h: 40pt), rgb("#e4efc9"), body)
#let joke(body) = sidebar("JOKE", sprite-fit("hero", "idle", w: 40pt, h: 40pt), rgb("#f6dd9a"), body)
#let warn(body) = sidebar("WARNING", sprite-fit("hero", "sick", w: 40pt, h: 40pt), rgb("#f1b8a8"), body)

// Screen width for the walk-through.
#let shot-s = 32mm

// Medium screen width, and the large one for an annotated screen.
#let shot-m = 40mm
#let shot-l = 56mm

// A round letter to point at a spot on a screen.
#let mark(l) = box(width: 11pt, height: 11pt, radius: 5.5pt, fill: rgb("#f6dd9a"),
  stroke: 1.2pt + ink, align(center + horizon, text(font: head-font, size: 6pt, l)))
// A screen with letters on it. `marks` are (letter, fx, fy), fractions of the
// screen's size.
#let annotated(name, marks, width: shot-l) = box({
  screen(name, width: width)
  for (l, fx, fy) in marks {
    place(top + left, dx: 4pt + (width - 1pt) * fx, dy: 4pt + (width - 1pt) * fy, mark(l))
  }
})

#let content-marks = (
  ("A", 0.40, 0.45), ("B", 0.16, 0.73), ("C", 0.02, 0.24),
  ("D", 0.90, 0.17), ("E", -0.07, 0.10), ("F", -0.07, 0.88))
#let gap-s = 3mm

// A plain ruled table for word lists.
#let word-table(see: false, ..cells) = table(
  columns: if see { (auto, 1fr, auto) } else { (1fr, 1fr) }, stroke: 0.5pt + ink, inset: 3.5pt, align: left, ..cells)

// The hero hatching: three egg frames, then the happy pet.
#let egg-strip() = align(center, grid(columns: 4, column-gutter: 3mm, align: bottom + center,
  ..range(3).map(i => sprite-fit("hero", "egg", frame: i, w: 70pt, h: 70pt)),
  sprite-fit("hero", "happy", w: 70pt, h: 70pt)))

// A numbered step of the walk-through; `art` (optional) sits beside the text.
#let step(body, art: none) = {
  counter("step").step()
  let num = box(width: 9mm, text(font: head-font, size: 9pt, context counter("step").display()))
  let text-col = grid(columns: (9mm, 1fr), num, body)
  block(above: 4mm, breakable: false, if art == none { text-col } else {
    grid(columns: (1fr, auto), column-gutter: 4mm, align: top, text-col, art)
  })
}

// The hero, big enough to sit beside a paragraph.
#let hero-pic(anim) = sprite-fit("hero", anim, w: 70pt, h: 70pt)

// A row of hero sprites with captions: (anim, caption) pairs.
#let pic-row(items) = grid(columns: items.map(_ => 1fr), column-gutter: 3mm, align: bottom + center,
  ..items.map(i => stack(spacing: gap-s, hero-pic(i.at(0)), text(size: 8pt, i.at(1)))))

// A keycap for the button diagram.
#let keycap(label, round: false, fill: white) = box(
  width: 17mm, height: 9mm, radius: if round { 4.5mm } else { 1.5mm },
  stroke: 2pt + ink, fill: fill,
  align(center + horizon, text(font: head-font, size: 5.5pt, label)),
)

// The badge's controls: the five-way joystick on the left, the two thumb
// buttons on the right.
#let pad-card() = {
  pic()
  let gap = box(width: 17mm, height: 9mm)
  chunky(grid(columns: (1fr, auto, 1fr), align: horizon + center, column-gutter: 4mm,
    grid(columns: 3, gutter: 1.5mm, align: horizon + center,
      gap, keycap("UP"), gap,
      keycap("LEFT"), keycap("FIRE", round: true, fill: rgb("#f6dd9a")), keycap("RIGHT"),
      gap, keycap("DOWN"), gap),
    box(width: 1.5pt, height: 28mm, fill: ink),
    stack(spacing: 4mm, keycap("EXE", round: true, fill: rgb("#e4efc9")),
      keycap("CAN", round: true, fill: rgb("#f1b8a8")))),
    fill: white.transparentize(50%))
}

// A chapter. `name` is its label ("ch-care"), so `@ch-care` works. The
// heading starts a new page and carries the chapter number in the pixel font
// and a sprite of the hero.
//
// `art: true` (the appendix chapters, whose generated tables run for pages
// with no room for a picture) marks the chapter as an art chapter: from its
// first page to the last content page, the page header draws a small roster
// pet and counts as the page's picture (see `art-header`).
#let chapter(name, title, art: false) = {
  pagebreak(weak: true)
  if art { [#metadata(none) <art-chapter>] }
  [#heading(level: 1, numbering: "1", title) #label(name)]
}

// The page header: empty, except on the pages of an art chapter, where a small
// pet (one of the roster, a different one on each page) sits at the right.
// `pic()` inside `sprite` makes it the page's picture.
#let art-header() = context {
  let p = here().page()
  let starts = query(<art-chapter>).map(m => m.location().page())
  let ends = query(<content-end>)
  if starts.len() > 0 and p >= starts.first() and ends.len() > 0 and p <= ends.first().location().page() {
    let pet = roster.pets.at(calc.rem(p, roster.pets.len()))
    align(right + horizon, sprite-fit(pet.prefix, "idle", w: 40pt, h: 24pt))
  }
}

// One roster pet: idle and happy faces, its name, the hero marked.
#let pet-card(p) = {
  let is-hero = p.prefix == roster.hero.prefix
  block(breakable: false, chunky(
    align(center, {
      grid(columns: (1fr, 1fr), column-gutter: 2mm, align: bottom + center,
        ..sprite-set(p.prefix, ("idle", "happy"), w: 66pt, h: 34pt))
      block(height: 1mm)
      text(font: head-font, size: 6.5pt, p.name)
      if is-hero {
        parbreak()
        text(size: 8pt)[Walks you through this guide as #hero.]
      }
    }),
    fill: if is-hero { rgb("#f6dd9a") } else { white.transparentize(50%) },
    inset: 4pt,
  ))
}

#let roster-grid(extra) = block(above: 3mm, grid(columns: 2, column-gutter: 5mm, row-gutter: 2mm,
  ..roster.pets.map(pet-card), extra))

// An unnumbered chapter-style page (Contents, Meet the pets).
#let chapter-plain(title, outlined: true) = {
  pagebreak(weak: true)
  heading(level: 1, outlined: outlined, title)
}

// Show rule for the whole book.
#let booklet(body) = {
  set document(title: "BornPets Player's Guide")
  set page(
    paper: "a5",
    margin: (x: 14mm, y: 16mm),
    fill: cream,
    header: art-header(),
    footer: context {
      let p = here().page()
      if p > 1 {
        align(center, text(font: head-font, size: 7pt, fill: ink, str(p)))
      }
    },
  )
  set text(font: body-font, size: 10.5pt, fill: ink, lang: "en")
  set par(justify: true, leading: 0.65em)
  set heading(supplement: "Chapter")
  show heading: set text(hyphenate: false)
  show heading: it => if it.level == 1 {
    block(
      width: 100%,
      above: 0pt,
      below: 1em,
      stroke: (bottom: 2.5pt + ink),
      inset: (bottom: 6pt),
      grid(columns: (1fr, auto), align: (left + horizon, right + horizon), column-gutter: 8pt,
        {
          if it.numbering != none {
            text(font: head-font, size: 20pt, counter(heading).display(it.numbering))
            v(3pt)
          }
          text(font: head-font, size: 13pt, fill: ink, it.body)
        },
        sprite-fit("hero", "idle", w: 60pt, h: 62pt)),
    )
  } else {
    block(above: 1.2em, below: 0.8em, text(font: head-font, size: 9pt, fill: ink, it.body))
  }
  show raw: set text(font: screen-font, size: 0.9em)
  body
}

// The cover. The start screen (start.png) is not pet art, so the cover shows
// the roster's idle sprites instead: the hero big, the others below.
#let cover() = {
  set page(footer: none)
  pic()
  align(center + horizon, {
    chunky(
      {
        v(6mm)
        text(font: head-font, size: 24pt, "BornPets")
        v(5mm)
        text(font: head-font, size: 9pt, "The official player's guide")
        v(8mm)
        sprite-fit("hero", "idle", w: 140pt, h: 125pt)
        v(5mm)
        grid(columns: (1fr,) * 3, column-gutter: 3mm, row-gutter: 4mm, align: bottom + center,
          ..roster.pets.filter(p => p.prefix != roster.hero.prefix)
            .map(p => sprite-fit(p.prefix, "idle", w: 95pt, h: 60pt)))
        v(5mm)
        pixel-heart(size: 6pt)
        v(2mm)
      },
      fill: white.transparentize(60%),
    )
  })
  pagebreak()
}

// Closes the book: pads with "Notes" pages so the total, back cover included,
// is a multiple of 4, then the back cover with the colophon.
// `notes-art(i)` returns the picture for the i-th Notes page: a roster pet
// (cycling), so the picture check passes.
#let finish(notes-art: i => sprite-fit(roster.pets.at(calc.rem(i, roster.pets.len())).prefix, "happy", w: 240pt, h: 200pt)) = {
  [#metadata(none) <content-end>]
  context {
    let last = query(<content-end>).first().location().page()
    let pad = calc.rem(4 - calc.rem(last + 1, 4), 4)
    for i in range(pad) {
      pagebreak()
      heading(level: 1, outlined: false, "Notes")
      align(center, notes-art(i))
    }
  }
  pagebreak()
  set page(footer: none)
  align(center + horizon, {
    pixel-heart(size: 6pt)
    v(6mm)
    text(font: head-font, size: 7pt, [Build #sys.inputs.at("commit")])
    [#metadata(sys.inputs.at("commit")) <colophon-commit>]
  })
  context [#metadata(here().page()) <guide-end>]
}
