ELF = target/thumbv7em-none-eabihf/debug/embassy
BIN = target/thumbv7em-none-eabihf/debug/embassy.bin

# App flash base matches the ACTIVE slot in memory.x (after the embassy-boot bootloader)
FLASH_BASE = 0x0000D000

.PHONY: fw sim flash monitor bl bl-flash dfu-flash

# Build the app firmware (debug)
fw:
	cargo fw

# Build and flash the app firmware via SWD
flash:
	cargo fw
	probe-rs download --chip nRF52840_xxAA $(ELF)

# Run the simulator
sim:
	cargo sim

# Monitor RTT output (app)
monitor:
	probe-rs attach --chip nRF52840_xxAA target/thumbv7em-none-eabihf/debug/embassy

# Monitor RTT output (bootloader)
monitor-bl:
	probe-rs attach --chip nRF52840_xxAA bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader

# Build the bootloader
bl:
	cd bootloader && cargo bl

# Build and flash the bootloader via SWD (do this once on a fresh device).
# A full chip erase is required first so that the embassy-boot state region
# at 0xC000 is 0xFF (empty). Without this, stale bytes from the previous
# Adafruit/S140 flash content at that address are misread as a DFU state
# and the bootloader panics.
#
# Uses `probe-rs download` (not `probe-rs run`) so make exits cleanly after
# programming without waiting for the firmware — the bootloader blinks red
# and waits indefinitely when no app is present, so `run` would never return.
bl-flash:
	probe-rs erase --chip nRF52840_xxAA
	cd bootloader && cargo bl
	probe-rs download --chip nRF52840_xxAA \
	    bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader
	@echo "Bootloader programmed. Run 'make flash' to install the app."

# Flash the app firmware over USB DFU.
# Hold the execute button while powering on to enter DFU mode (red LED blinks).
dfu-flash:
	cargo fw
	arm-none-eabi-objcopy -O binary $(ELF) $(BIN)
	dfu-util -D $(BIN)
