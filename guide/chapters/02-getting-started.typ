#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#chapter("ch-start")[Getting started]

This is your first ten minutes. Follow the steps in order. At the end your
pet has hatched, eaten, had a drink and gone to bed. #hero is waiting.

== The buttons

#pad-card()

#block(above: 3mm)[
- *Up, Down, Left, Right* move the cursor. On the pet screen, Right at the
  last icon flips to the next screen, and Left stops at the first icon.
- *Fire* is the joystick pressed in. *EXE* does the same job. Both pick and
  confirm.
- *CAN* is Cancel. It steps back or closes a menu, and the menus list it as
  #w("Cancel").
]

The badge keeps several screens in a row. The game is one of them. Until
your pet hatches, Left and Right flip the screen from the game too. If you
get lost, keep pushing Left or Right until the pet shows up.

== Step by step

#step(art: screen("start", width: shot-s, caption: [The start screen.]))[
  *Find the game.* Push Left or Right until the BornPets screen shows. There
  is no pet yet, so you see the start picture. Press Fire.
]

#step(art: screen("select-pet", width: shot-s, caption: [Pick a pet.]))[
  *Choose your pet.* The screen is called #w("Choose your Pet"). Up and Down
  move the highlight. The list is whatever is installed on your badge. Press
  Fire, as #w("Fire to confirm") says. Cancel does nothing here, because a
  pet is not optional.
]

#step(art: screen("select-mode", width: shot-s, caption: [Three modes.]))[
  *Choose how money works.* Next comes #w("Play with money?"). You choose
  again for every new pet.
  - #w("With money"): the pet starts with #v("START_HAX"). Food, drinks and
    medicine have prices. See @ch-money.
  - #w("Without money"): no prices and no HAX to earn, so no worries.
  - #w("Hard mode (US)"): money is on, and some prices are higher. See
    @ch-hard. Bring a flag.
]

#step(art: screen("egg", width: shot-s, caption: [The egg, counting down.]))[
  *Wait for the egg.* It wobbles for #v("HATCHING_TICKS"). The buttons do
  nothing while it counts down, except Left and Right, which still flip the
  screen. Go and get a coffee.
]

#step(art: screen("naming", width: shot-s, caption: [Naming.]))[
  *Name your pet.* When the egg hatches, the badge opens #w("Name your Pet").
  A random Danish or Dutch name is already filled in, so you can accept it.
  Press EXE. The badge asks #w("Save name?") and shows #w("EXE = save").
  Press EXE again. Any other button backs out, as #w("any key = back") says,
  and an empty name shows as #w("(empty)").

  To type your own, use the joystick. Push towards #w("A-I"), #w("J-R") or
  #w("S-Z"). Push again to pick a group of three letters. Then push Left or
  Right to move along them and press Fire to take one. The bottom line
  reminds you: #w("</>: move  Fire: select"). #w("CMD") holds
  Backspace, special characters, numbers, Shift, Clear and the submit key.
  #w("Cancel = quit") leaves the screen. A name can be up to
  #v("PET_NAME_MAX") letters long. With an external keyboard plugged in, the
  joystick picker gives way to a plain hint: #w("- Keyboard -"),
  #w("type to enter") and #w("Enter = save").
]

#step(art: screen("hatched", width: shot-s, caption: [Hatched.]))[
  *Say hello.* Your pet is alive. The word under it is its status. A calm pet
  says #w("content"). The icons are your actions. Up and Down switch between
  the two rows of icons, and Left and Right pick one.
]

#step(art: screen("menu-feed", width: shot-s, caption: [The Feed menu.]))[
  *Feed it.* Pick the #w("Feed") icon on the bottom row and press Fire. A
  list of food opens. Press Fire on #w("Salad"). With money on, the price is
  drawn after the name, like #w("<price>H"). @ch-care says what each food
  does.

  *Give it a drink.* Pick the glass (#w("Drink")) on the top row, press Fire
  and choose #w("Water"). @ch-care has the details.
]

#step(art: grid(columns: 2, column-gutter: 3mm, align: top,
  screen("menu-rest", width: shot-s, caption: [The Rest menu.]),
  screen("asleep", width: shot-s, caption: [Goodnight.])))[
  *Put it to bed.* Pick the #w("Rest") icon, press Fire and choose
  #w("Sleep"). The menu also has #w("Relax") and #w("Wash"). #w("Wake up") is
  greyed out until your pet is asleep. A sleeping pet says #w("asleep").
]

Sleep has no timer: the pet sleeps until you
wake it, or until it wakes itself. @ch-care tells you more. You did it:
hatched, fed and asleep. Well done.

== Classic or Casual

The game has two rule sets, and this guide describes Classic. To change it,
go to the Main screen, then #w("Bornagotchi"), and step the #w("Mode:") row. The change
takes effect after a reboot. @ch-hackers has the details.

== When something is wrong

#grid(columns: (1fr, auto), column-gutter: 4mm, align: top,
  [
    - #w("No pets installed") and #w("Check PETS.CFG"): the list of pets
      comes from a file called PETS.CFG on the badge's USB drive. It is
      missing or has no usable rows. Put it back and the pets return.
      @ch-hackers shows how.
    - #w("No sprites on flash"): the badge was flashed without its pictures.
      The game still runs, but the pet area shows this note instead of your
      pet.
  ],
  hero-pic("sick"),
)

#joke[If a pet looks at you funny, check the status word first. Then check
yourself.]
