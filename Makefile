ELF         = target/thumbv7em-none-eabihf/debug/embassy
BIN         = target/thumbv7em-none-eabihf/debug/embassy.bin
BIN_REL     = target/thumbv7em-none-eabihf/release/embassy.bin
ELF_REL     = target/thumbv7em-none-eabihf/release/embassy
ELF_REL_DBG = target/thumbv7em-none-eabihf/release-debug/embassy
ELF_HWTEST  = target/thumbv7em-none-eabihf/release-hwtest/hwtest

# App flash base matches the app slot in memory.x (ORIGIN in memory.x = 0xD000)
FLASH_BASE = 0x0000D000

.PHONY: fw fw-release fw-release-debug fw-game fw-game-release fw-mesh fw-mesh-release \
        fw-hwtest flash-hwtest run-hwtest monitor-hwtest \
        sim flash flash-release flash-release-debug run-release-debug \
        flash-game flash-mesh \
        monitor monitor-release-debug bl flash-bl dfu-flash dfu-flash-release \
        fw-watch flash-watch

# ---------- Full build (game + mesh) ----------

fw:
	cargo fw
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-release:
	cargo fw-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash:
	cargo fw
	probe-rs download --chip nRF52840_xxAA $(ELF)

flash-release:
	cargo fw-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# Release codegen (full LTO, opt-z) WITH defmt symbols + debug info —
# use to diagnose release-only crashes via RTT.
fw-release-debug:
	cargo fw-release-debug
	@arm-none-eabi-size $(ELF_REL_DBG) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

# Flash + attach RTT (decodes defmt, prints stack trace on panic /
# hardfault).  Ctrl-C to detach.
run-release-debug:
	cargo fw-release-debug
	probe-rs run --chip nRF52840_xxAA --always-print-stacktrace $(ELF_REL_DBG)

# Flash only (no attach).
flash-release-debug:
	cargo fw-release-debug
	probe-rs download --chip nRF52840_xxAA $(ELF_REL_DBG)

# Attach to an already-flashed release-debug binary.
monitor-release-debug:
	probe-rs attach --chip nRF52840_xxAA --always-print-stacktrace $(ELF_REL_DBG)

# ---------- Game only (no mesh) ----------

fw-game:
	cargo fw-game
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-game-release:
	cargo fw-game-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-game:
	cargo fw-game
	probe-rs download --chip nRF52840_xxAA $(ELF)

flash-game-release:
	cargo fw-game-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# ---------- Mesh only (no game) ----------

fw-mesh:
	cargo fw-mesh
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-mesh-release:
	cargo fw-mesh-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-mesh:
	cargo fw-mesh
	probe-rs download --chip nRF52840_xxAA $(ELF)

flash-mesh-release:
	cargo fw-mesh-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# ---------- Watch app ----------

fw-watch:
	cargo fw-watch

flash-watch:
	cargo fw-watch
	probe-rs download --chip nRF52840_xxAA $(ELF)

flash-watch-release:
	cargo fw-watch-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# ---------- Factory hardware test (standalone, no bootloader) ----------

fw-hwtest:
	cargo fw-hwtest
	@arm-none-eabi-size $(ELF_HWTEST) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-hwtest: fw-hwtest
	probe-rs erase --chip nRF52840_xxAA
	probe-rs download --chip nRF52840_xxAA $(ELF_HWTEST)
	probe-rs reset --chip nRF52840_xxAA

# Flash + attach RTT console (defmt log stream over SWD).
run-hwtest: fw-hwtest
	probe-rs erase --chip nRF52840_xxAA
	probe-rs run --chip nRF52840_xxAA $(ELF_HWTEST)

# Attach to an already-running hwtest (no flash, no reset).  Only useful
# while the chip is actively logging — after the test finishes the CPU
# parks in WFI and probe-rs misreports it as "locked up core".  For
# flash-and-watch use `make run-hwtest`.
monitor-hwtest:
	probe-rs attach --chip nRF52840_xxAA $(ELF_HWTEST)

# ---------- Simulator ----------

sim:
	cargo sim

# ---------- Game simulation ----------

simulate-game:
	cargo run --bin simulate_game --features simulator

# ---------- Monitor / debug ----------

monitor:
	probe-rs attach --chip nRF52840_xxAA --always-print-stacktrace target/thumbv7em-none-eabihf/debug/embassy

bl-monitor:
	probe-rs attach --chip nRF52840_xxAA bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader

# ---------- Bootloader ----------

bl:
	cd bootloader && cargo bl

flash-bl:
	probe-rs erase --chip nRF52840_xxAA
	cd bootloader && cargo bl
	probe-rs download --chip nRF52840_xxAA \
	    bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader
	@echo "Bootloader programmed. Run 'make flash' to install the app."

# ---------- USB DFU ----------

dfu-flash:
	cargo fw
	arm-none-eabi-objcopy -O binary $(ELF) $(BIN)
	dfu-util -w -D $(BIN)

dfu-flash-release:
	cargo fw-release
	arm-none-eabi-objcopy -O binary $(ELF_REL) $(BIN_REL)
	dfu-util -w -D $(BIN_REL)
