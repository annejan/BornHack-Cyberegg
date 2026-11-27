#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/social.typ": *
#chapter("ch-realm")[The Unicorn Realm]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

Every pet leaves in the end. Some leave because they are old, some because
they were hungry, and some because you pressed a button. The ones that leave
are not forgotten. They go to the Unicorn Realm.

#align(center, realm-row())

== Visiting

Open the Main screen, then #w("Bornagotchi"), and pick #w("Unicorn Realm"),
the last row. Up and Down scroll the list. Any other button closes it. A
badge with no past pets says #w("No past pets yet").

The Realm keeps the last #v("REALM_MAX_PETS") pets, the newest first. When it
is full, the oldest is dropped to make room. It lives in its own save, apart
from the pet, so nothing a pet does can change it.

*Each pet gets two lines.* The first line is the name, the kind and the age,
like #w("<name> (<kind>) - <d>d <h>h"). A pet that never got a name shows
its kind and generation instead, like #w("<kind> Gen <n> - <d>d <h>h"). The
age does not count time spent frozen (@ch-offtime). The second line holds the pet's three traits:
#w("V:<n>% C:<n>% R:<n>%"), for vitality, curiosity and resilience. They are
the same three that #w("Rolled stats") shows (@ch-care). Four pets fit on
a screen. When there are more, a count like #w("<n>/<m>") sits in the
corner.

#tip[A name goes in the book when the pet leaves. Choose it with care. It
cannot be changed there.]

== How a pet gets in

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    - *It left.* A pet that goes Gone (@ch-mood) is written to the book the
      next time the badge saves. You do not have to do anything.
    - *You reset it.* #w("Reset Pet") in the #w("Bornagotchi") menu asks
      #w("Reset pet?"). Say yes and the kind picker opens. When you confirm
      the new egg there, the pet that is alive is written to the book.
      Back out of the picker and the pet stays. Use it only when you mean it.
    - *An egg is never written.* A pet that never hatched did not live, so it
      leaves no entry.
  ],
  hero-pic("critical"))

== Generations

The first pet on a badge is generation zero. Each new egg is one more than
the pet before it. The number shows in the Realm for unnamed pets, and goes
out in the beacon your friends hear (@ch-friends).

*A new egg after Gone.* When a pet has gone, the screen shows a farewell
sign. In red it says #w("Press Fire") and #w("for a new egg"). Fire opens the
same choice as the first time: the kind of pet, then the money mode
(@ch-start), then the egg.

#gone-to-egg()

*What starts fresh.* Everything about the pet: the needs, the conditions,
the mood and the diary. A new set of traits is rolled. The
balance goes back to #v("START_HAX"). The Battle record is zero again.
*What stays.* The Realm, your friends list and the Classic or Casual
choice. The new egg is not caught up on the old pet's off-time
(@ch-offtime).

#joke[Pets are not replaced, only followed. The next one will not know the
first one. It will not wash either.]
