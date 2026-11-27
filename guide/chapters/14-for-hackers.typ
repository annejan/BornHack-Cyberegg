#import "../lib/booklet.typ": *
#import "../lib/data.typ": *
#import "../lib/hackers.typ": *
#chapter("ch-hackers", art: true)[For hackers]
#set text(size: 10pt)
#set par(leading: 0.6em, spacing: 0.9em)
#set list(spacing: 0.55em)

The badge shows up as a USB drive when you plug it in. Its name starts with
CYBR. Two small text files on that drive change how BornPets plays. This
appendix shows how, and ends with a table of every number you can change.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    Every file here is plain text. Edit it, eject the drive properly so the
    write reaches the badge, and reboot. The badge reads these files at boot
    only. A file you edit while the pet is running changes nothing until the
    next start.
  ],
  hero-pic("playing"))

== Classic or Casual

Two rule sets ship with the badge. *Classic* is the one this guide describes.
*Casual* is gentler: slower decay, longer lifetimes and more relief from care
(@ch-money has the Casual boxes). Switch on the Main screen: open
#w("Bornagotchi") and step the #w("Mode:") row. A `*` after the mode
name means the change is saved but not running yet. Reboot, and it takes
over.

#pic-row((("idle", [Classic.]), ("happy", [Casual.])))

== `BPETS_V3.CFG`, the balance file

The badge ships with a file called `BPETS_V3.CFG` in the root of the drive.
It lists every setting as a comment, with its Classic value and its Casual
value. Nothing in it is switched on. Updating the badge's files never
overwrites a copy you have edited.

#cfg-example()

- *The format.* One `KEY=VALUE` per line. Spaces around the equals sign are
  fine. A `#` starts a comment, so a line that starts with `# ` is off.
- *Uncomment a line to use it.* Remove the `# ` at the start and put your own
  number after the equals sign. The `# Casual` note at the end of the line
  is a comment, and can stay.
- *Unknown keys are ignored.* A typo in a key, or a key from a later
  firmware, is skipped without a word. Keys are not case sensitive.
- *The value is a plain whole number.* No units, no decimals, no minus
  sign. A line with anything else in the value is dropped, silently.
- *Values are clamped.* A number too big for its field is cut down to the
  biggest the field holds. A few settings have a tighter range, and band
  edges stay inside the percent range.
- *A bad band pair falls back.* Several settings come in a pair, a low edge
  and a high edge, and the low one must be the smaller. When a pair is out
  of order, both edges of that pair go back to the preset's values. Other
  pairs are untouched.
- *A reboot applies it.* The badge reads the file once, at boot.

#grid(columns: (1fr, auto), column-gutter: 5mm, align: top,
  [
    *The star.* When at least one value differs from the preset, the pet's
    name on the stats screens gets a small `*` after it. No star means
    nothing was applied. If the star is there and you did not expect it, open
    the file and look.
  ],
  screen("stats-body", width: shot-s, caption: [The stats.]))

#warn[Wild values are allowed. A huge drain really will starve the pet
before you reach the eject button. To go back to a preset, delete the file
from the drive and reboot.]

*Time in the table.* The badge counts in ticks, and #v("TICKS_PER_MINUTE")
ticks make a minute. The table says which unit each row uses. A row marked
raw is a plain number in the engine's own scale: a name ending in PM is a
per-minute rate or a share, in thousandths, one ending in DIV is a divisor, and one
ending in MULT is a multiplier.

== `PETS.CFG`, the roster

The list of pets also lives in a file, `PETS.CFG`. The firmware has no pets
of its own. The list in this file *is* the roster, in the order of the
pick-a-pet screen. An empty or missing file leaves the badge with nothing to
pick, and the pick screen says #w("No pets installed") and
#w("Check PETS.CFG").

#pets-example()

One line per pet. The number is the pet's sprite slot, which picks the
picture files (they are named after it). The name is what the screen shows,
cut at #v("PETS_CFG_NAME_CAP") letters, plain ASCII only. A `#` starts a
comment. Two slots are reserved for the badge and ignored, and so are
numbers past the last slot. A slot listed twice is not added twice: the later
line renames the pet. A bad line is skipped.

The shipped file already fills every slot the firmware has. So the useful
edits are renaming a pet, reordering the list, or removing one. A pet's looks never
change the rules: all pets play the same.

== Back to the old firmware

Going back is safe. The new firmware never writes or deletes the old
firmware's saves, and never touches its balance file:

- the old balance file, `BORNPETS.CFG`, is never read, written or deleted. It
  stays on the drive and the old firmware keeps reading it;
- the old pet save, kept under the key `state`, and the old name, under
  `name`, are read only to carry the pet over. That happens only when there is
  no `state_v3` save or `name_v3` name yet. They are never written or deleted,
  so they stay as they were;
- the new firmware saves its own copy under `state_v3` and `name_v3`.

The first time the new firmware starts on a badge with an old pet, it reads the old save
and carries the pet over, name included. From then on it only writes the new
keys. Flash the old firmware again and the old pet is there, as it was on the
day you left. Whatever the pet did under the new firmware stays there. The Unicorn Realm
list is shared by both, because its format did not change.

== More to read

- `USER_GAMES.md` has the full game guide, the mini-games and the picture
  file rules.
- `README.md` covers the badge and the firmware.
- `QUICKSTART.md` takes a new badge from the box to a pet, and lists the
  files on the drive.

== Every setting

Each row is a key you may put in `BPETS_V3.CFG`. The value is the bare
number the file wants. Casual is in *bold* where it differs from Classic.

#cfg-table()
