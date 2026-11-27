#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#chapter-plain("Meet the pets")

Pick one of these when you start. Each shows two faces: calm, and happy.

#roster-grid(joke[Nobody asked the seastar for its opinion on the menu. It is
still the best listener on the badge.])

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    The jellyfish has swum off to the Unicorn Realm. On a badge whose pet was
    a jellyfish, that pet moves to the Realm, with its record, the first
    time the badge starts without the jellyfish pictures. The screen says
    #w("Moved to the") #w("Unicorn Realm"), and #w("Press Fire") starts a
    new egg. The screen shows a unicorn under a rainbow above the words. The
    jellyfish stays in the Realm list under its name.
  ],
  screen("to-the-realm", width: shot-m, caption: [The jellyfish has gone.]))
