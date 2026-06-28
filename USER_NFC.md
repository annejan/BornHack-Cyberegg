# NFC — User Guide

The back of the badge has an NFC antenna. Tap a phone or a station reader to the badge to interact.

## Two things happen on a tap

### 1. Phone reads the URL

Any standard NFC reader (Android default, iOS) sees a `https://badge.team` URL with your badge ID. Tapping with the OS reader just opens that page in a browser. Harmless.

### 2. BadgeCtl runs a station command

A phone running the **BadgeCtl** companion app (loaded with the matching event private key) can send signed commands when held to the badge. These are typically used at event "stations" where you can boost your pet's stats.

Available signed commands:

| Command                   | Effect on your BornPet            |
| ------------------------- | --------------------------------- |
| `more food`               | sets **hunger** to 0              |
| `more drugs`              | sets **sick** to 0                |
| `more inspiration`        | sets **drained** to 0             |
| `sleep like a bear`       | sets **tired** to 0               |

A short toast appears on the badge confirming what happened. Each command has a 5-minute cooldown — tapping twice in quick succession does nothing the second time.

## What you need to do

Nothing on the badge. The badge is always ready. Just hold the back of the badge close to the reader for a moment.

The reader side needs:

- The **BadgeCtl** app installed
- The matching Ed25519 private key bundled in (BornHack staff have it for stations)

Third-party / random NFC reader apps cannot issue these commands — they don't have the key. They just see the public URL.

## If you want to run your own station

You need to rebuild the badge firmware with your **own** Ed25519 public key, then sign the matching commands with your private key in your reader app. See [NFC_README.md](NFC_README.md) for the protocol spec and signing recipe.
