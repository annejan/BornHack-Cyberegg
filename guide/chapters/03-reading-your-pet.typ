#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#chapter("ch-reading")[Reading your pet]

Your pet cannot talk. It has a status line, a buzzer, a speech bubble and a
diary instead. This chapter teaches you to read all four.

== The screen

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  annotated("content", content-marks),
  [
    - *A* is the pet area. Your pet lives here and moves about.
    - *B* is the status line. It says how your pet is doing.
    - *C* is the toast corner. A short note shows here after you do
      something, and when something goes wrong.
    - *D* is your HAX balance. It only shows in the money modes. Without
      money, the spot stays empty.
    - *E* and *F* are the two rows of icons. The top row holds the info,
      hibernate, exercise and drink icons. The bottom row is the care row.
      @ch-start showed you how to use them.
  ])

#block(above: 3mm)[A hibernating pet is frozen, so its status line is empty.
The battery meter sits in the top corner too, above the balance.]

#tip[Toasts are polite. They go away by themselves. The status line stays
until the problem is fixed.]

== Sky and floor

Behind your pet there is a little world. It tells you the time of day and
how clean the room is, before you read a single word.

=== The sky

#grid(columns: (1fr, 1fr), column-gutter: 4mm, align: top + center,
  screen("day", width: shot-m, caption: [Day: white, with a sun and a cloud.]),
  screen("night", width: shot-m, caption: [Night: dark, with a moon.]))

By day the sky is white, with a sun and a small cloud. At night the sky is a dark
dither, with a moon and a few stars. Night runs from #v("BG_NIGHT_FROM_MIN")
to #v("BG_NIGHT_TO_MIN") by the badge's clock. Both times are the hour of
the day, counted from midnight. If the clock was never set, the badge cannot
tell, and the sky stays plain white.

The night sky is only a picture of the hour. It is *not* the pet's bedtime. A
pet can be up and about at night, and it can nap in broad daylight.

The sky costs no extra screen flashes. It changes when the clock crosses
the line, with an ordinary quiet redraw, not a flash.

=== The floor

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  screen("floor", width: shot-m, caption: [A filthy floor.]),
  [
    A dirty pet leaves a mess. Things pile up on the floor as dirt rises,
    one more each time dirt reaches a line:

    #word-table(see: false,
      table.header([*On the floor*], [*Dirt reaches*]),
      [Crumbs], [#v("FLOOR_CRUMBS_DIRT_PCT")],
      [A peel], [#v("FLOOR_PEEL_DIRT_PCT")],
      [A can], [#v("FLOOR_CAN_DIRT_PCT")],
      [A sock], [#v("FLOOR_SOCK_DIRT_PCT")],
      [A pizza box], [#v("FLOOR_PIZZA_DIRT_PCT")],
    )

    Each item has its own spot, and it never moves. A wash clears them all
    at once.
  ])

=== The cross

When the pet is poorly, a small red cross shows in the sky. It comes with
the red status line.

A hibernating pet is frozen, and it has no sky at all.

== The status line

The line under the pet is the most important thing on the screen. It has
one or two words. Here is how the badge picks them.

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  screen("two-words", width: shot-m, caption: [Two words.]),
  [
    Every problem pulls the pet's mood down. The badge ranks the pulls and
    names the biggest one. A problem that pulls only a little is not named
    at all. If nothing is named, you get an *info word* instead.

    The *first word* is, in this order:
    + a *danger word*, if there is one;
    + else the biggest problem;
    + else a prompt, if there is one.

    Then come the *prompts*, #w("no insulin") and #w("needs rehab"). They
    say that a lasting condition needs a drug. A prompt takes the second
    place before another problem can.

    The *second word* is the next biggest problem. It shows only when its
    pull is more than #v("SECOND_WORD_UNDER_PCT"). There is never a third
    word.
  ])

If both words are too wide for one line, the second word drops below the
first.

=== Red means danger

A pet in danger gets a *red* status line. The danger words are checked in a
fixed order, and the first one that fits wins:

+ #w("starving"): fullness under #v("NEED_DANGER_LO_PCT").
+ #w("parched"): hydration under #v("NEED_DANGER_LO_PCT").
+ #w("wasted"): booze over #v("BOOZE_DANGER_PCT").
+ #w("filthy"): dirt over #v("DIRT_DANGER_PCT").
+ #w("poorly"): health under #v("HEALTH_DANGER_PCT").

#grid(columns: (1fr, 1fr), column-gutter: 4mm, align: top + center,
  screen("starving", width: shot-m, caption: [Danger: red.]),
  screen("wasted", width: shot-m, caption: [Booze over the line.]))

The whole line turns red, second word and all. The danger word always comes
first.

#tip[A pet in demo mode never shows a red status line. The danger word is
still there, in black, but nothing turns red and nothing buzzes. The only
red line a demo pet has is its call for help, in
the last section of this chapter.]

=== The info words

When nothing is wrong, the line shows one of five words. The badge tries
them in this order:

#grid(columns: (1fr, auto), column-gutter: 4mm, align: top,
  [
    + #w("asleep"): the pet is in bed.
    + #w("sore"): strain is over #v("SORE_PCT"), and no injury says it
      already.
    + #w("tipsy"): booze is at least #v("TIPSY_PCT").
    + #w("well rested"): the pet slept well and the bonus is still on.
    + #w("content"): all is well.
  ],
  screen("asleep", width: shot-m, caption: [Asleep.]))

=== Every status word

#grid(columns: (1fr, auto), column-gutter: 5mm, align: bottom,
  [Here is every word the line can show, what it says, and the chapter that
  tells more.],
  hero-pic("hungry"))

#let row(word, means, see) = (w(word), means, see)
#word-table(see: true,
  table.header([*Word*], [*What it says*], [*Read on*]),
  ..row("peckish", [Fullness is a bit low.], [@ch-needs]),
  ..row("hungry", [Fullness is under #v("HUNGRY_PCT").], [@ch-needs]),
  ..row("starving", [Fullness is in the danger zone. Red.], [@ch-needs]),
  ..row("stuffed", [Fullness is too high. Let it digest.], [@ch-needs]),
  ..row("thirsty", [Hydration is low.], [@ch-needs]),
  ..row("parched", [Hydration is in the danger zone. Red.], [@ch-needs]),
  ..row("waterlogged", [Too much water. No more drinks.], [@ch-needs]),
  ..row("sleepy", [Energy is low, and the pet is awake.], [@ch-needs]),
  ..row("exhausted", [Energy is under #v("EXHAUSTED_PCT").], [@ch-needs]),
  ..row("bored", [Fun is low, and the pet is awake.], [@ch-needs]),
  ..row("wired", [Fun is too high. The pet is overexcited.], [@ch-needs]),
  ..row("tipsy", [A little booze. An info word.], [@ch-needs]),
  ..row("sloppy", [Booze is above its comfort band.], [@ch-needs]),
  ..row("wasted", [Booze is in the danger zone. Red.], [@ch-needs]),
  ..row("grubby", [Dirt is above its comfort band.], [@ch-needs]),
  ..row("filthy", [Dirt is in the danger zone. Red.], [@ch-needs]),
  ..row("overweight", [Weight is too high.], [@ch-needs]),
  ..row("underweight", [Weight is too low.], [@ch-needs]),
  ..row("unfit", [Fitness is too low.], [@ch-needs]),
  ..row("hung over", [A hangover is running.], [@ch-conditions]),
  ..row("sore", [Strain is high. An info word.], [@ch-needs]),
  ..row("injured", [An injury is running.], [@ch-conditions]),
  ..row("groggy", [A groggy spell is running.], [@ch-conditions]),
  ..row("poorly", [Health is low. Red in the danger zone.], [@ch-mood]),
  ..row("no insulin", [A diabetic pet needs its medicine.], [@ch-conditions]),
  ..row("needs rehab", [A dependent pet needs rehab.], [@ch-conditions]),
  ..row("asleep", [In bed. An info word.], [@ch-care]),
  ..row("well rested", [A good sleep bonus. An info word.], [@ch-care]),
  ..row("content", [Nothing to name. All is well.], [@ch-mood]),
)

== The buzz

The badge has a buzzer, and it speaks up less than you might fear. For the
status line, it buzzes only when things get *worse*, and only when the
*first* word is one of these. A second word never buzzes. Think of a ladder
with four steps: calm, warning, danger and leaving.

#pic-row((("hungry", [Hungry: buzz.]), ("idle", [Calm: quiet.])))

- *Buzzes, as the first word:* #w("hungry"), #w("starving"), #w("thirsty"), #w("parched"),
  #w("exhausted"), #w("wasted"), #w("filthy"), #w("hung over"),
  #w("injured"), #w("groggy") and #w("poorly"). Any danger word buzzes.
- *Quiet:* every other word, such as #w("peckish"), #w("sleepy"),
  #w("bored") or the prompts.

The buzzer sounds when the pet steps *up* the ladder: from calm to warning,
from warning to danger, and from danger to leaving. It does not sound for
getting better, and it does not repeat while the pet stays on one step. It
stays silent right after the badge boots, so a pet that was already hungry
does not wake the room. A pet that has just gone gets a short, silly tune of
its own.

Two more buzzes do not come from the status line. The diabetes and
dependence alarms sound on *every boot*, unless the pet is hibernating.
@ch-conditions tells the full story.

A pet in demo mode is silent. It does not buzz for the status line, and it
does not sound the two boot alarms either.

To silence the game, go to the Main screen, then #w("Bornagotchi"). The
second row is the mute switch. It reads #w("Mute (On)") while the game is
muted and #w("Mute (Off)") while it is not. Muting silences the status
buzz, the two alarms and the tune for a pet that has gone.

#joke[The pet does not buzz for #w("peckish"). It is only peckish. It will
survive. It always says that.]

== The idle frame

The e-paper screen only uses power when the picture changes. So the badge
does not animate an idle pet. It draws *one frame every*
#v("IDLE_FRAME_SECS"). A calm pet shows one frame, waits, and shows the next.
That spares the battery and the panel.

*Actions animate at full speed.* When you feed, drink, heal, play, exercise,
medicate, take Ozempic or go to rehab, the pet moves quickly for those few
seconds. The egg animates quickly too.

The screen also redraws when something happens, such as a toast, a new status
word or a menu. So if your pet looks frozen, it is not. It is napping
between frames.

#pic-row((("idle", [Idle.]), ("happy", [Happy.]), ("sleeping", [Asleep.])))

Each of these looks, and every other one, gets one frame at a time. One
frame, then a long nap.

== The Hint row

Do not know what your pet wants? Ask it. Pick the Stats icon, then the
#w("Hint") row. A speech bubble opens with two lines. It is titled with the
pet's name: #w("<name> says"). A pet with no name says
#w("Your pet says").

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  screen("hint", width: shot-m, caption: [A hint.]),
  [
    The *first line* is the pet naming its state. For most words it reads
    #w("I'm <word>!"), for example "I'm starving!". The word is the first
    word of the status line. The special ones are:
    - #w("I'm diabetic!") for #w("no insulin"), and #w("I'm dependent!") for
      #w("needs rehab").
    - #w("Zzz...") when asleep, #w("I feel great!") when well rested, and
      #w("I'm happy!") when content.
    - A tired-out pet with nothing worse to say reads #w("I'm worn out!").
    - #w("I feel stiff.") from a demo pet that is awake, has only an info word
      to say, and has a fitness under #v("CEILING_FIT_STEP3_PCT"). The second
      line then asks for exercise. the last section of this chapter has the story.
    - #w("*wobble*") for an egg, #w("...") for a pet that has left, and
      #w("Brrr, frozen!") for a hibernating one.
  ])

The *second line* is what the pet wants you to do:

#word-table(see: false,
  table.header([*The pet says*], [*Do this*]),
  [#w("Please feed me.")], [Feed it.],
  [#w("Water, please.")], [Give it water.],
  [#w("Put me to bed.")], [Sleep.],
  [#w("Play with me!")], [Play with it.],
  [#w("Play a game!")], [Play a mini-game.],
  [#w("Let me relax.")], [Relax.],
  [#w("Wash me, please.")], [Wash.],
  [#w("Let's work out!")], [Exercise.],
  [#w("I need medicine.")], [Heal, for an injury or poor health.],
  [#w("I need insulin.")], [Give insulin, from the Heal menu.],
  [#w("I need rehab.")], [Rehab, from the Heal menu.],
  [#w("Defrost me!")], [Defrost, from the Hibernate menu. See @ch-offtime.],
)

Some words have nothing to ask for. The pet then says a fixed line:
#w("Let me digest.") when stuffed, #w("No more drinks.") when waterlogged,
#w("Hic! Thanks!") when tipsy, #w("Let me sleep.") when asleep and
#w("Thanks!") when well rested or content. An egg says #w("I'm hatching!")
and a pet that has left says #w("Your pet left.").

=== The pet never asks for the impossible

The pet only asks for something the menu would accept. If there are
several ways out, it asks for a clean one first, and it picks one with a
catch only when none is clean. When the menu would refuse every way, the
pet tells you why: #w("but <reason>"). The reasons are the ones the menu
shows, such as #w("too tired") or #w("can't afford"). @ch-care lists them
all.

#tip[The Hint row costs nothing, and the pet never lies. When in doubt,
ask.]

== The Diary

Something went wrong, and you missed it? Pick the Stats icon, then the
#w("Diary") row. The diary keeps the last #v("DIARY_LEN") causes, newest
first.

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  stack(spacing: gap-s,
    screen("cause-toast", width: shot-m, caption: [The toast.]),
    screen("diary", width: shot-m, caption: [The diary.])),
  [
    When something goes wrong, a *cause toast* shows in the toast corner. It
    has two lines: what went wrong, and why. It is silent. It is also
    written in the diary.

    Each diary entry has two lines, like the toast. The first starts with
    the pet's *age* at the time: #w("now"), #w("<n>m") for minutes,
    #w("<n>h") for hours and #w("<n>d") for days. The second line says why.

    - It holds #v("DIARY_LEN") entries. The oldest drops off.
    - Three entries fit a page. With more, there are two pages. The title
      reads #w("Diary 1/2") and #w("Diary 2/2"). Left and Right flip pages.
    - Any other button closes it. It always opens at the first page.
    - With no entries it says #w("Nothing yet"). A short diary shows only
      the title #w("Diary").
    - The diary is *not saved*. Reboot the badge, or hatch a new pet, and it
      starts empty.
    - The ages tick on by themselves while it is open.
  ])

If a full-screen view or a menu is open when something goes wrong, the
toast stays away. The cause still lands in the diary.

#joke[Never mind. Nobody reads their own diary after a bad night. Your pet
does not, either.]

== Demo mode on the game screen

In demo mode the pet looks after itself. @ch-care tells how to switch it on
and what the pet does with its day. Here we only read the screen. A demo pet
adds four things to it.

#grid(columns: (auto, 1fr), column-gutter: 5mm, align: top,
  stack(spacing: gap-s,
    screen("demo-note", width: shot-m, caption: [A note, and the demo mark.]),
    screen("demo-help", width: shot-m, caption: [The red call for help.])),
  [
    - *The demo mark.* A small black letter *D* sits under the battery meter
      in the top corner. It shows on the game screen only. A menu or a
      full-screen view, such as the diary, hides it. It is black, so it
      never costs a full refresh.
    - *The note.* When the pet has looked after itself, a one-line note
      stands where the status line stands. It says what the pet did. It
      stays while the status line is the same as it was when the pet acted.
      When the status line changes, or when you do something yourself, the
      status line is back.
    - *The call for help.* When health is under
      #v("DEMO_HELP_HEALTH_PCT"), the whole line reads #w("could use a hand")
      in red. It beats the note and the status line. It goes when health is
      back at that line or above.
    - *The hint.* An awake demo pet that has nothing worse to say, and is not
      fit, says #w("I feel stiff."). It asks for exercise, the one thing it
      never does by itself.
  ])

=== Every note

#word-table(see: false,
  table.header([*The note*], [*What the pet did*]),
  [#w("had an apple") #w("had a salad") #w("had some pizza")
   #w("had a Frikandel spec") #w("had some cake")], [It ate that food.],
  [#w("had some water") #w("had a coffee") #w("fancied a beer")], [It drank.
    Any beer reads #w("fancied a beer"), a thirsty one too.],
  [#w("had some wine") #w("had a whiskey")], [The badge can word them. The pet
    never reaches for them by itself, so you will not meet them.],
  [#w("one too many...")], [A drink took its booze over the high line. It
    stands in place of the drink note.],
  [#w("napping")], [It lay down for a short sleep.],
  [#w("off to bed")], [It went to bed.],
  [#w("up and about")], [It got up.],
  [#w("having a wash")], [It washed.],
  [#w("had a play")], [It played, or finished a mini-game by itself.],
  [#w("had a rest")], [It relaxed.],
  [#w("took medicine")], [It healed itself.],
  [#w("did a shift")], [It went to work (#w("Only pets")) for HAX, when HAX is on
    and the balance was low.],
  [#w("took insulin") #w("went to rehab")], [A diabetic pet took its insulin,
    and a dependent one went to rehab.],
)

#tip[A note is a short story, not a log. Only the last thing the pet did is
there. The diary (above) stays about what went wrong, and the pet's own
acts do not go in it.]

=== When you step in

The pet does not mind help. Anything you do for it costs the usual HAX (the
pet pays for its own care too, @ch-money), and the pet then leaves it alone for #v("DEMO_PAUSE_MIN"), so it does not undo
what you did. Your action also wipes the note.

If your care made things better, nothing went wrong with it, and it did not
wake the pet grumpy, a toast thanks you: #w("thanks!") on the first line and
#w("that helped") on the second. Only your help earns it. The pet's own
causes stay quiet on the screen, and the diary keeps them.
