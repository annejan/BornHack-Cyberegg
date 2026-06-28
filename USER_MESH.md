# Mesh, PMs & Channels — User Guide

Your badge has a LoRa SX1262 radio and speaks the **MeshCore** mesh protocol. Other badges, MeshCore phones, and standalone repeaters all show up as peers.

Four carousel screens are mesh-related: **PMs**, **Channel**, **Adverts**, and (indirectly) **My QR**.

## Make sure you're on the same network

Three things must match across all badges in the local mesh:

1. **LoRa preset** — frequency, bandwidth, spreading factor. Default is **BornHack 2026** (preset baked in firmware). Change in **Main → Settings → LoRa Radio**.
2. **Public channel key** — automatically shared via the preset.
3. **Antenna** — make sure the LoRa antenna is connected.

If you can't see anyone else's adverts after a minute, double-check the preset.

## Adverts (Advert screen)

Whenever a badge / phone / repeater is alive on the mesh it periodically broadcasts an **advert** — its public name, identity hash and capabilities. Your badge logs these as they arrive.

| Key                | Action                                         |
| ------------------ | ---------------------------------------------- |
| Up / Down          | scroll the advert list                         |
| EXE / Fire         | save the highlighted advert as a contact       |
| CAN                | back                                           |
| Left / Right       | next carousel screen                           |

Saved contacts appear under **Main → Contacts** — see [USER_CONTACTS.md](USER_CONTACTS.md).

## Private messages (PMs)

The **PMs** screen is your private inbox. Each row is one peer who has messaged you.

| Marker | Meaning                                       |
| ------ | --------------------------------------------- |
| `●`    | last heard from less than 5 minutes ago        |
| `*`    | favourite                                      |
| `+`    | discovered, not saved as a contact yet         |
| `R`    | repeater                                       |
| `#`    | room / channel server                          |
| `S`    | sensor                                         |

### Reading and replying

| Key                | Action                                          |
| ------------------ | ----------------------------------------------- |
| Up / Down          | scroll list / thread                            |
| EXE / Fire         | open thread / start a reply                     |
| CAN                | back to inbox                                   |

Replies use the on-screen keyboard with ~70 emoji rendered as 13×13 bitmaps. Use the joystick to pick characters, **EXE** to commit, **CAN** to backspace.

> The inbox holds up to **32 messages across 16 peers** in RAM. Saved contacts and their threads persist; unsaved peers vanish on reboot. Mark-read state is also per-boot.

## Channels (group chat)

The **Channel** screen is for group / room messages — same protocol, broadcast scope. Each row is one channel (e.g. the default `Public` channel that ships with the preset).

Controls are the same as PMs. Anyone on the same preset hears everyone's messages in a public channel.

## My QR

The **My QR** screen renders your mesh identity as a QR code. Hand it to someone else's MeshCore phone or another badge for instant pairing — no need to wait for an advert to be heard.

## Pinging and visibility

- When another badge pings you with the special `blinkme` mesh command, your LED briefly flashes in the requested colour. Fun way to find friends in a crowd.
- Your badge also sends adverts itself, so other people can see you.

## Sound, sleep, and battery

Mesh radio is the largest battery drain. To save power:

- **Main → Settings → MeshCore** has options to mute notification sounds.
- Closing the lid / dimming has no effect — the e-paper draws no power once shown.

## Pairing via Bluetooth (companion app)

If you'd rather chat via your phone's MeshCore app: install MeshCore on Android / iOS or open `https://app.meshcore.nz/`. Unplug USB (BLE only runs when USB is disconnected) — the badge advertises as `Cyber Ægg XXYY`. Bond once with the 6-digit passkey shown on the e-paper, then everything (contacts, chat, settings) is also reachable from the app.
