#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/social.typ": *
#chapter("ch-offtime")[Off-time and hibernation]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

Your pet lives only while the badge is on. This chapter is about the time
when it is not: what the pet misses, what it catches up on, and the one
button that stops the clock on purpose.

== While the badge is off

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *No time passes.* Switch the badge off and the pet is paused with it.
    It does not get hungry in the drawer. The badge has no backup battery for
    its clock, so it cannot count the hours you were away.

    *Until the clock is known.* After a boot the badge does not know what
    time it is. Until it learns, the pet simply picks up where it stopped,
    and the minutes the badge is on count as usual. Two things set the clock:
    the MeshCore phone app over Bluetooth, or a time advert from a repeater
    in range. #raw("USER_WATCH.md") has the steps, under "Setting the time".
  ],
  hero-pic("sleeping"))

#offtime-diagram()

*The catch-up.* The first time the clock becomes known after a boot, the
badge works out the gap: the time from the last save to the moment the badge
was switched on. It then plays that gap in one go, with the same rules as
normal time. The gap is capped at #v("CATCH_UP_MAX_MIN"). A badge left in a
drawer for a week costs the pet no more than that.

- *Once per boot.* A later re-sync of the clock does nothing more.
- *The same rules.* Needs drain, conditions run and an egg keeps hatching. A neglected pet can fall ill or start to leave. It can even be
  gone when the screen wakes. A hungry pet in the drawer is a hungry pet
  at the catch-up.
- *Clock never set.* The badge keeps a lower bound for the time it was on,
  so a reboot before the clock arrives does not lose the time off. It never
  counts the time the badge was running twice.
- *No clock, no gap.* A badge that never knew the time before it was
  switched off has nothing to measure from, so that gap is not counted.
- *A new egg starts clean.* An egg is never caught up on the old pet's gap.

#tip[Set the clock when you boot. Skip it and you give the pet a free
holiday. That is kind, but it is also not in the rules.]

== Hibernation

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    Hibernation is a pause that you choose. Open the top-row icon next to
    Stats. The menu has #w("Hibernate") and #w("Defrost"), and the demo
    switch (@ch-care). Both are free and
    instant. Hibernate freezes the pet. Defrost thaws it. It works on an egg
    and on a pet that is leaving, but not on one that has gone.
  ],
  screen("hibernating", width: shot-m, caption: [Frozen.]))

*What freezes.* Everything the clock drives. The needs stop. So do mood,
health, age, every condition timer, every cooldown, the hatching countdown
and the leaving clock. A frozen pet does not get better or worse. The badge
only counts the time as frozen.

*Off-time while frozen.* The catch-up still runs, but all it does is add to
the frozen time. Nothing in the pet changes. Hibernation also survives a
reboot, because the badge saves it the moment you press the row.

*What you can still do.* Only two icons open: Stats and Hibernate. The other
icons do nothing. In Stats, #w("View stats"), #w("Hint"), #w("Diary") and
#w("Rolled stats") work. #w("Friends") does not. The status line is empty,
and the pet says #w("Brrr, frozen!") with #w("Defrost me!") under it. Press
Hibernate on a frozen pet and the row says #w("hibernating").

*What still reaches it.* A friend over the mesh and a magic place still
land on a frozen pet (@ch-friends). The badge applies them whatever the
pet is doing.

*Defrost.* The toast says #w("waking up"). Time runs again from that moment,
as if the pause never happened.

== Alarms and the freeze

The diabetes and dependence alarms (@ch-conditions) sound on every boot for
as long as the pet has the condition. *A hibernating pet is the exception.*
While it is frozen, neither alarm fires. If the alarm has not sounded yet
this boot, it sounds at Defrost. The mute switch still silences only the buzz
(@ch-reading). When it fires, the alert on the screen shows. A frozen pet gets neither buzz nor alert.

#pic-row((("hibernating", [Frozen.]), ("sick", [Alarm on boot.]), ("happy", [Thawed.])))

#warn[Hibernation does not cure anything. A pet with a condition is still
ill when it thaws, and a leaving pet still has its clock when the clock runs
again. It buys time. It does not fix the cause.]

#joke[A frozen pet never has a birthday. It is the only known cure.]
