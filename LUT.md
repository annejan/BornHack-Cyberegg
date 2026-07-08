# Custom e-paper LUT (`LUT.CFG`)

The badge normally drives its e-paper panel with the waveform LUT it
reads from the panel's own OTP at boot. You can override that with a
**calibrated waveform** — e.g. one exported from the `ssd1675-calibration`
tool — by dropping a `LUT.CFG` file onto the badge's USB drive. No
reflash needed.

## How to use

1. Plug the badge in via USB — it appears as a small removable drive
   ("FAT12 Storage").
2. Copy `LUT.CFG` (see format below) to the root of that drive.
3. Eject and reset the badge. On boot it loads the LUT and drives the
   panel with it.

To go back to the panel's built-in waveform, delete `LUT.CFG` (or hold
**Fire** at boot — see recovery).

## File format

Plain text, `KEY=VALUE` per line, `#` starts a comment — the same style
as `PETS.CFG` / `BORNPETS.CFG`. Only two keys are read; anything else is
ignored, so you can trim a calibration-tool export down by hand.

```
# CyberAegg EPD custom LUT
variant=A                 # A = SSD1675 / SSD1675A, B = SSD1675B
band_lut=08992144...      # 214 hex chars = one 107-byte LUT unit
```

- **`variant`** — `A` or `B`. **Must match your panel.** The badge
  auto-detects its own panel variant and *ignores the file* on a
  mismatch: an A-panel LUT on a B panel (or vice-versa) uses the wrong
  row layout and drive voltages and can blank or stress the display.
- **`band_lut`** — the 107-byte register-0x33 LUT unit as hex (exactly
  214 hex chars). This is the `band_lut` field from the calibration
  tool's JSON export; the trailer timing/voltage bytes are already baked
  into it. The same waveform is applied to all temperature bands, so a
  custom LUT does not track temperature the way the OTP tables do.

The multi-stage `stage_luts` and the staged-drive `controls` from the
calibration tool's full export are **not** used by this path — the badge
firmware runs the single-LUT refresh engine. Only `variant` + `band_lut`
matter here.

## Recovery — if a LUT renders badly

Hold **Fire** (the joystick centre press) while the badge boots. This
forces the safe OTP waveform and ignores `LUT.CFG` for that boot, so you
can always get a readable screen back even if a custom LUT blanked it.
Delete or fix the file, then reboot normally.

The badge also rejects a `LUT.CFG` that is malformed, the wrong length,
or the wrong variant, falling back to OTP automatically.
