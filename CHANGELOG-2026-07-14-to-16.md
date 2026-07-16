# Changelog — 2026-07-14 → 2026-07-16

CyberÆgg badge firmware (`Ranzbak/bornhack-firmware-2026`) and its e-paper
driver submodule (`Ranzbak/ssd1675`).

## Added
- **I2C keyboard text entry** (#124) — optional Nicolai-Electronics I2C
  keyboard on the Qwiic bus for typing names/messages; falls back to the
  on-screen joystick picker when absent. Includes Shift/Alt one-shot
  toggles with on-screen badges and an alt-symbol layer matching the
  keyboard silkscreen.
- **"Disable Game" actually works** (#132, closes #131) — the menu item
  was a no-op stub; now a real persisted toggle (label flips
  *Disable Game* ⇄ *Enable Game*). Hides the pet screen from navigation,
  survives reboot, and blocks the NFC pet-jump while disabled.

## Fixed
- **Text-entry confirm before submit** (#130, closes #126) — pressing
  Execute at the very start of name entry used to commit the (prefilled)
  name and exit instantly, with no way back. Now shows a
  *Save name? / EXE = save / any key = back* confirmation.
- **SSD1675B "whole image inverted"** (ssd1675 #6 → firmware #127) —
  `reset()` no longer pulses RES# mid-waveform; waits the in-flight
  waveform out first on B panels, which was latching the complement image
  after a cancelled refresh.
- **A-panel snappy redraw regression** (ssd1675 #8 → firmware #128) — the
  #6 wait was applied to *all* panels, stalling the interrupt-driven fast
  redraw on A panels (rapid screen switches stopped updating until a
  waveform drained). Now variant-gated: A pulses RES# immediately, B keeps
  the correctness wait.

## Performance
- **−6.2 KiB static RAM** (#125) — dropped the redundant `sent_pending`
  snapshot buffer in the e-paper driver (ssd1675 #7, −5776 B) and shrank
  oversized USB descriptor buffers (256→64/32 B).

## Chore
- Dropped an unused `defmt::todo` import — clears the last build warning
  (#129).
- Submodule bumps to carry the ssd1675 fixes onto `main` (#127, #128).

---

### Also landed Tue 2026-07-14 (sponsor slideshow, by at-boy)
- Chain boot slideshow groups — no mid white flash / second intro (#122).
- Restore "No assets found" block-forever screen at first boot (#123).
- Ship screen wording: "FACTORY TESTED" → "FIELD TESTED".

---

_Verified on hardware (A panel) via DFU/JLink; badge runs `main` @ 528f8e0._
