# CyberÆgg — FAQ

Common "wait, why does it do that?" questions from badge holders. For getting
started see [QUICKSTART.md](QUICKSTART.md); for the one-page reference see
[POCKET_CARD.md](POCKET_CARD.md).

## Why does the red LED keep blinking, especially on the pet screen?

The RGB LED does a short one-shot flash every time the e-paper screen refreshes.
On most screens the picture is static, so this is rare. The **BornPet** screen is
different: the pet has a slow "breathing" idle animation, so the badge wakes up
every few seconds to draw the next frame — and each of those refreshes flashes
the LED. Frames can look almost identical, so it feels like the LED is blinking
"for no reason."

It is not a fault and not (usually) a mesh message.

**Note:** the *Ignore blink* setting (Config → MeshCore) only silences the LED
flash for **incoming mesh messages** — it does not stop the per-refresh flash.
If the flashing bothers you, park on a static screen (see below).

## Which screen uses the least battery?

E-paper itself draws almost nothing once an image is on it — the power goes into
*refreshes*. So the most efficient screens are the ones that never redraw on
their own:

| Screen | Refreshes on its own? | Power |
| ------ | --------------------- | ----- |
| **My QR**, **Name**, **Token**, **Calendar** | No — drawn once, then idle until you press a button | **Lowest** |
| Messages / Channels / Contacts | Only when relevant mesh traffic arrives | Low |
| **Main**, **Watch** | Once a minute (to update the clock) | Medium |
| **BornPet** | Every few seconds (idle animation) | **Highest** |

**Battery tip:** to make a charge last as long as possible, leave the badge on
**My QR** or **Name**. They sit completely idle — no periodic refresh, no LED
flashing — until you touch a button. (Bonus: My QR is also the handiest screen
to leave up so people can scan you into the mesh.)

## Why does the BornPet screen change by itself?

The pet is animated — it breathes, blinks, and reacts (hatching, feeding,
etc.). The badge wakes on a timer to advance the animation, which is why that
screen updates and flashes the LED without you doing anything.

## The screen looks inverted or ghosted — is it broken?

E-paper occasionally needs a full-screen refresh (a brief flash where the whole
panel cycles) to clear ghosting and re-seat the ink; this is normal. If the
image looks *inverted* (colours swapped) and stays that way, flip to another
screen and back to force a redraw. Persistent inversion on the red-capable
("B") panels was a known bug — make sure you are on current firmware.

If ghosting has built up and you don't want to wait for the automatic
full-refresh or hunt for a screen to flip through, there's a hidden combo that
forces a clean de-ghost on demand, on whichever screen is currently showing:
press **Down, Down, Up, Up, Right, Left, Right, Left, Fire**. The panel goes
solid black, then solid white, then redraws the real screen — same idea as
manually flipping screens, just immediate and without needing a second screen
to flip to.

If ghosting is a recurring problem on your particular panel rather than a
one-off, there's also a persistent setting for it: **Settings → De-ghost
menus**. Turned on, every menu, text box, and mini-game close automatically
runs that same black → white de-ghost cycle, not just the game's normal
"redraw whatever changed" refresh. It's off by default — the extra full-panel
cycling is slower and flashes every time a menu opens or closes, which most
badges don't need.

## How do I put the badge in DFU mode to flash it?

Hold the **EXE** (Execute) button while powering the badge on. The bootloader
shows a **red blinking** LED (idle) → **solid blue** (flashing) → **solid green**
(done — power-cycle). Then drag-drop a `.uf2`, or flash the `.bin` with
`dfu-util`. See [QUICKSTART.md](QUICKSTART.md) for the full recovery flow.

## How do I charge it? Is there a charge light?

USB-C in any port charges it — there is no separate charge LED. The battery
level shows as an icon on the status bar / watch face. Unplug USB to re-enable
BLE pairing.

## How does someone add me as a mesh contact?

Open **Config → MeshCore → My QR** (or press **Left** a few times from Main to
reach the QR screen). They scan the code with the MeshCore app and you're added
— no key typing.
