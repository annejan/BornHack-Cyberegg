#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/needs.typ": *
#import "../lib/care.typ": *
#import "../lib/social.typ": *
#chapter("ch-friends")[Friends, battles and stations]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

Pets are not alone at a hacker camp. This chapter covers the three things
that reach your pet from outside: the friends it meets over the mesh radio,
the battles it can fight with them, and the magic places that hand out a
treat when you tap the badge.

== Friends

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    Every badge with this firmware joins a private mesh channel and sends out
    a small beacon from time to time. It carries the pet's kind, its name,
    its generation and its fighting numbers. When your badge hears another
    badge's beacon, that pet becomes a *friend*. Your own beacon, echoed
    back, is ignored.

    - *A new friend.* The toast says #w("new friend!"). The pet gains fun:
      #v("DOSE_FRIEND_BIG_FUN_PCT").
    - *A friend again.* After #v("FRIEND_BOOST_COOLDOWN") since the last
      boost, hearing the same friend gives #v("DOSE_FRIEND_SMALL_FUN_PCT") fun
      and the toast #w("+happy"). Sooner than that, the badge just notes
      that it saw them.
  ],
  hero-pic("happy"))

A tired pet gets the fun doubled, the same as for play (@ch-care). A sleeping
pet is not woken. A boost needs no menu and costs nothing. The badge must be on, with its
mesh radio running.

*The list.* The #w("Friends") row of the Stats menu opens it. It keeps the
last #v("FRIENDS_MAX") friends, the one seen most recently on top. When it is
full, the oldest drops off. The list is empty at first, and says
#w("No friends met yet"). Up and Down move the cursor. Fire opens the
details: the friend's name and kind, #w("Met <d>d <h>h ago") and
#w("Last seen <d>d <h>h ago"), four bars (#w("Pow"), #w("Grd"), #w("Spd")
and #w("HP")) and the record between you two, as #w("Battles: <w>W-<l>L").
The foot of the list says #w("Fire for details"). The details card is
titled #w("Friend Details") and its corner says #w("Back"): any button goes
back. If the cursor has no friend under it, the card says
#w("No friend selected").

The "ago" times do not count the time the badge was off, so a friend can
look younger than they are.

== Battles

#versus()

Two pets fight. Your badge plays the whole fight at once. It uses the
fighting numbers each pet sends out in its beacon.

*Where.* #w("Battle: <form>") is the first row of the Play menu, in both money
modes. The word after the colon says how your pet feels about a fight against
an average pet. It is one of four forms:

#tbl((auto, 1fr),
  table.header([*The form*], [*What it means*]),
  [#w("top form")], [Your pet should win most fights against an average pet.],
  [#w("in form")], [A fair fight. Your pet has the edge.],
  [#w("off form")], [Your pet is worse than average today.],
  [#w("poor form")], [Your pet will probably lose. Look after it first.],
)

*The picker.* Fire on the Battle row opens a list of your friends. The title
bar shows your pet's own record, #w("Battle <w>W-<l>L"): wins, then losses.
The record belongs to this pet. A new egg starts it at zero. A demo pet shows
#w("Demo <w>W-<l>L") instead. Each row reads #w("<name> ~<n>% <age>"): the
friend's #w("<name>"), then an estimate of your chance to win, marked with a
tilde. It comes from the friend's last beacon, and the age says how old that
beacon is: #w("now"), or a count in minutes (#w("<n>m")), hours (#w("<n>h")) or
days (#w("<n>d")). A friend of the other league shows #w("<name> demo") or
#w("<name> player"), with #w("demo") or #w("player") as the league word,
and no chance. Under the
rows the badge shows your pet's own form word, and below it up to two words
for what helps (#w("+<word>")) and what hurts (#w("-<word>")) your pet today.
The foot of the list says #w("Fire to challenge"). With no friends it says
#w("No friends") and #w("to battle yet"). A friend whose badge has not sent
its numbers gives #w("Friend not ready"). A badge built without the mesh radio
says #w("No radio on badge").

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
*The challenge.* Fire on a friend sends them a challenge. Your screen says
#w("waiting for <name>") and waits for up to twenty seconds. On the other
badge the buzzer goes. Its screen shows a pre-battle card, with the friend's
name and #w("<name> challenges you!") (on a long name, just
#w("challenges you!")). Your pet's numbers are on the left and
theirs on the right, as bars labelled #w("Pow"), #w("Grd"), #w("Spd"),
#w("Sta") and #w("Foc"). Under them is the hit points line,
#w("HP <n>") on the left and a bare #w("<n>") on the right. The line
#w("<n>% to win") is your chance. Against a friend who is not in the room it
shows the estimate, #w("~<n>% to win"). #w("Fire: fight Cancel: no") tells the
player what to press: Fire to fight, Cancel to decline. The player has
fifteen seconds to answer.
  ],
  screen("pre-battle", width: shot-m, caption: [A challenge.]))

If nobody answers, the fight is played anyway against the friend's last
beacon, and the result card shows as usual. Only when there is no usable
beacon does the waiting card say #w("no answer"), and then no fight happens.
If the other player declines, the waiting card says
#w("declined"), and the pet that said no is a bit sulky: mood
-#v("BATTLE_DECLINE_MOOD_PCT") for #v("BATTLE_DECLINE_MIN"). A challenged pet that
is asleep still gets woken by Fire. A frozen pet answers too, but the fight does
not touch it. Demo pets fight only demo pets, and they accept by themselves
after a short while.

*What the numbers mean.*
- #w("Pow") is power: how hard the pet hits. A sturdy, cheerful pet hits
  harder. A sore, sad or craving pet hits softer.
- #w("Grd") is guard: how well it takes a hit. Resilience and body weight feed
  it. Drink makes it reckless.
- #w("Spd") is speed: who strikes first. Fitness and a light body feed it.
  A full belly, a heavy body and a groggy pet slow it down.
- #w("Sta") is stamina: how many rounds the pet fights at full strength.
  Energy, fitness and good food feed it. Past its stamina, a pet gets slower
  and weaker.
- #w("Foc") is focus: how often a hit lands. Curiosity and a calm, clean pet
  feed it. Drink, boredom, too much fun and dirt cost aim.
- #w("HP") is hit points: how much it can take. Health, vitality and body
  weight feed it.

*The words.* A helping word is shown with a plus, a hurting word with a
minus. The pre-battle card and the picker show up to two of them for each pet.
First the words that help, then the ones that hurt.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  tbl((auto, 1fr), // digits-ok: layout
    table.header([*Helps (+)*], [*What it does*]),
    [#w("well rested")], [Slept well: quicker, better aim, more stamina.],
    [#w("heavy")], [A big body: more power and guard. Slower, and tires sooner.],
    [#w("tipsy")], [A little drink gives courage: a bit more power.],
    [#w("cheerful")], [A happy pet hits harder.],
  ),
  hero-pic("happy"))

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  tbl((auto, 1fr), // digits-ok: layout
    table.header([*Hurts (-)*], [*What it does*]),
    [#w("injured")], [Power and speed are cut.],
    [#w("sloppy")], [Too much drink: poor aim and a weak guard.],
    [#w("hung over")], [Slower, poor aim and tires sooner.],
    [#w("too light")], [Too little body: weak hits.],
    [#w("hungry")], [Tires sooner.],
    [#w("thirsty")], [Tires sooner, and the aim suffers.],
    [#w("craving")], [A dependent pet that wants its drink: poor aim, weak hits.],
    [#w("wired")], [Too much fun: wild aim.],
    [#w("stuffed")], [A full belly: slower.],
    [#w("bored")], [Not into it: poor aim.],
    [#w("down")], [A sad pet: weaker hits, poor aim.],
    [#w("groggy")], [Woken too early: slower, poor aim.],
    [#w("worn out")], [Short nights: tires sooner.],
    [#w("waterlogged")], [Too much water: slower.],
    [#w("sore")], [Strained muscles: weaker hits.],
    [#w("filthy")], [A dirty pet: poor aim.],
  ),
  sprite-fit("hero", "sick", w: 50pt, h: 50pt)) // digits-ok: layout

*How a fight goes.* The faster pet strikes first. The fight lasts ten rounds
at most. Damage has a little luck in it, and a pet that is past its stamina
slows down. If both pets still stand after ten rounds, whoever has more HP
left wins.

*The result.* The fight is shown as a short picture of both pets, and then a
card, titled #w("Battle Result"), with two HP bars and a banner,
#w("YOU WON!") or #w("YOU LOST"). Your side is labelled #w("You") and the
friend's side carries the friend's name, or #w("Friend") when the name is
empty. The card says #w("chance was <n>%"): the chance you had (from the
beacon, when the friend did not answer). When one thing made the loser lose,
a line says so, such as #w("You were <word>")
or #w("They were <word>"), with a word from the table, or
#w("You ran out of steam") or #w("They ran out of steam"). The card says
#w("Any button to close"), and any button does. If no fight has been played
yet, it says #w("No result yet").

*The prize.* In money mode the first win of the day pays
#v("BATTLE_PRIZE_FIRST_HAX") and each win after it #v("BATTLE_PRIZE_LATER_HAX"),
shown as #w("+<n> HAX"). A fight against a beacon older than
#v("BATTLE_PRIZE_BEACON_MIN") pays nothing and costs nothing (see below). Without money there is no prize.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
*A fight costs.* A fight against a beacon older than
#v("BATTLE_PRIZE_BEACON_MIN") costs nothing and pays no prize. It still counts
in the record, and the Battle row still rests. Any other fight, win or lose,
makes the pet pay: energy -#v("BATTLE_ENERGY_PCT"),
water -#v("BATTLE_WATER_PCT"), fun +#v("BATTLE_FUN_PCT"), dirt
+#v("BATTLE_DIRT_PCT"), and some strain. A tired, hungry or thirsty pet strains
more, and a well rested one less. The pet also gets bruises: the more HP it
lost, the more health (up to #v("BATTLE_BRUISE_MAX_PCT")). A win lifts the mood
by #v("BATTLE_MOOD_PCT"), a loss lowers it by the same. Push the strain over
the line and the pet tears a muscle: the cause toast says
#w("torn muscle") and #w("in a fight"). Then the Battle row rests for
#v("BATTLE_COOLDOWN"). The same two pets also need to wait
#v("BATTLE_PAIR_COOLDOWN_MIN") before they fight again.
  ],
  gym-pic())

*The other badge.* If you challenged and nobody answered, the result is also
sent out. A badge that hears it, in direct radio range, counts the fight the
other way round and shows #w("won a battle!") or #w("lost a battle"). That pet
did not fight, so there is no prize, no cost and no rest. The record is all
it changes. The result does not travel across repeaters.

*Who cannot fight.* The Battle row is grey, and the cursor skips it, when the pet is
#w("asleep"), #w("busy"), #w("hibernating"), #w("still an egg") or
#w("pet is gone"). A pet that is #w("injured"), #w("too weak") or
#w("too drunk") cannot fight either. #w("wait <time>") says the rest has not
run out. A demo pet shows #w("demo pets only") against a player, and a player
shows #w("no demo pets") against a demo pet.

== Stations: the magic places

At events there are *stations*: a food stand, a medic and a rest corner.
Hold the back of the badge to the station's reader for a moment. The
reader sends a short signed phrase. The badge checks the signature, and
only then does the pet get its treat. A phone with a random NFC app
cannot do it. It has no key.

The badge ignores the tap unless it has a pet that has not gone. An egg
counts. A frozen pet counts too. The phrase is matched whatever its
capitals. Anything else is ignored without a word.

#tbl((auto, 1fr, auto),
  table.header([*Phrase*], [*What the pet gets*], [*Toast*]),
  [#raw("more food")],
  [Fullness and water lifted to #v("STATION_FOOD_FLOOR_PCT"), never lowered.],
  [#w("Food bonus!")],
  [#raw("more drugs")],
  [Health up to the pet's own ceiling. Booze down to at most
   #v("STATION_BOOZE_CAP_PCT"). Hangover, injury and groggy cleared.
   Dependence and diabetes stay.],
  [#w("Heal bonus!")],
  [#raw("sleep like a bear")],
  [Energy full. Groggy and worn out cleared. The next waking forgets any
   oversleeping, so it cannot be groggy. It does not put the pet to sleep.],
  [#w("Sleep bonus!")],
)

#station-row()

*The cooldown.* Each phrase rests for #v("STATION_COOLDOWN"). The first tap
after a boot always works. A tap too soon does nothing to the pet. It shows a
countdown in the toast corner, like #raw("wait 4:50"). A reboot clears the
rests.
