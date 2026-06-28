# Contacts — User Guide

The **Contacts** list (under **Main → Bornagotchi → Contacts** or jump there from the **Adverts** screen) shows everyone your badge has heard or knows: nearby strangers, saved friends, repeaters and rooms.

## List view

| Key                  | Action                                                      |
| -------------------- | ----------------------------------------------------------- |
| Up / Down            | scroll                                                      |
| **Up** at the top row | open the **Filter** chooser (All / Favorites / People / Repeaters / Rooms / Sensors) |
| EXE / Fire           | popup: PM, Info, Add, Save / Unsave, Forget                 |
| CAN                  | back                                                        |
| Left / Right         | switch carousel screen                                      |

## Markers next to a name

| Marker | Meaning                                       |
| ------ | --------------------------------------------- |
| `●`    | last heard less than 5 min ago                |
| `*`    | favourite (always at the top)                 |
| `+`    | discovered, not yet saved                     |
| `R`    | repeater                                      |
| `#`    | room / channel server                         |
| `S`    | sensor                                        |

## Popup actions

Push **EXE / Fire** on any contact to open the popup:

- **PM** — open the message thread (see [USER_MESH.md](USER_MESH.md)).
- **Info** — shows hex identity prefix, last-heard time, advert capabilities.
- **Add** / **Save** — persist this contact in flash so it survives reboot.
- **Unsave** — drop it from flash (it stays in the discovery cache until reboot).
- **Forget** — remove now, even from the discovery cache.

> The **discovery cache** holds up to 32 unsaved peers in RAM. Each reboot it's empty until adverts arrive again. **Save** anyone you actually want to keep.

## Sending a message

Pick a contact → PM. Type with the on-screen keyboard (5-way joystick + EXE). About 70 emoji are available as 13×13 bitmaps. **CAN** backspaces. Send by selecting the on-screen "send" key (or holding EXE).

## Why some contacts vanish

Anyone you have **not** Saved is RAM-only. On reboot:

- The discovery cache is empty.
- PMs from those peers are gone.
- Mark-read state resets.

If you want the chat history to stick around, hit **Save** as soon as you've spoken to someone.

## Limits

- 32 discovery slots (RAM-only)
- 32 messages across 16 peers in PM inbox (RAM-only)
- Saved-contacts limit is set by the firmware build — generally far higher than 32

If discovery is full, the oldest unsaved entry is evicted to make room for a new advert.
