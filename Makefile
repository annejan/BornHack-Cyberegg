ELF         = target/thumbv7em-none-eabihf/debug/embassy
BIN         = target/thumbv7em-none-eabihf/debug/embassy.bin
BIN_REL     = target/thumbv7em-none-eabihf/release/embassy.bin
ELF_REL     = target/thumbv7em-none-eabihf/release/embassy

# App flash base matches the app slot in memory.x (after the 64 K bootloader)
FLASH_BASE = 0x00010000

.PHONY: fw fw-release sim flash flash-release monitor bl bl-flash dfu-flash

# Build the app firmware (debug)
fw:
	cargo fw

# Build the app firmware (release)
fw-release:
	cargo fw-release

# Build and flash the debug firmware via SWD
flash:
	cargo fw
	probe-rs download --chip nRF52840_xxAA $(ELF)

# Build and flash the release firmware via SWD
flash-release:
	cargo fw-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# Run the simulator
sim:
	cargo sim

# Monitor RTT output (app)
monitor:
	probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/debug/embassy

# Monitor RTT output (bootloader)
bl-monitor:
	probe-rs attach --chip nRF52840_xxAA bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader

# Build the bootloader
bl:
	cd bootloader && cargo bl

# Build and flash the bootloader via SWD (do this once on a fresh device).
# Full chip erase is done first to clear any stale content.
# Uses `probe-rs download` (not `probe-rs run`) so make exits cleanly after
# programming without waiting for the firmware — the bootloader blinks red
# and waits for DFU when no valid app is present.
bl-flash:
	probe-rs erase --chip nRF52840_xxAA
	cd bootloader && cargo bl
	probe-rs download --chip nRF52840_xxAA \
	    bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader
	@echo "Bootloader programmed. Run 'make flash' to install the app."

# Flash the app firmware over USB DFU.
# Hold the execute button while powering on to enter DFU mode (red LED blinks).
# -w: wait up to 10 s for the device to appear (so you can plug in after make).
dfu-flash:
	cargo fw
	arm-none-eabi-objcopy -O binary $(ELF) $(BIN)
	dfu-util -w -D $(BIN)


dfu-flash-release:
	cargo fw-release
	arm-none-eabi-objcopy -O binary $(ELF_REL) $(BIN_REL)
	dfu-util -w -D $(BIN_REL)
