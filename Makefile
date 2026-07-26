ELF         = target/thumbv7em-none-eabihf/debug/embassy
BIN         = target/thumbv7em-none-eabihf/debug/embassy.bin
BIN_REL     = target/thumbv7em-none-eabihf/release/embassy.bin
ELF_REL     = target/thumbv7em-none-eabihf/release/embassy
ELF_REL_DBG = target/thumbv7em-none-eabihf/release-debug/embassy
ELF_HWTEST  = target/thumbv7em-none-eabihf/release-hwtest/hwtest

# App flash base matches the app slot in memory.x (ORIGIN in memory.x = 0xD000)
FLASH_BASE = 0x0000D000

# Output dir (in this firmware project) for built flasher artifacts. The
# flashers are pointed here explicitly via their own path arguments — no
# cross-project directory coupling.
FW_OUT = firmware

# Bootloader release ELF the SWD mass-programmer (mass-flash-bl) flashes.
BL_ELF = bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader

.PHONY: fw fw-release fw-release-debug fw-game fw-game-release fw-mesh fw-mesh-release \
        fw-organizer fw-organizer-release flash-organizer flash-organizer-release dfu-flash-organizer \
        fw-1680 fw-1680-release flash-1680 flash-1680-release \
        fw-1680-game fw-1680-game-release flash-1680-game \
        fw-1680-mesh flash-1680-mesh dfu-flash-1680 dfu-flash-1680-release \
        fw-1680-organizer fw-1680-organizer-release flash-1680-organizer \
        fw-hwtest flash-hwtest run-hwtest monitor-hwtest fw-hwtest-bin fw-full-bin \
        sim flash flash-release flash-release-debug run-release-debug \
        flash-game flash-mesh \
        monitor monitor-release-debug bl flash-bl dfu-flash dfu-flash-release \
        fw-watch flash-watch fw-bin-release bl-bin

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

# ---------- SSD1680 panel (mutually exclusive with the SSD1675 fw*/flash* targets) ----------
# A separate binary: only the SSD1680 driver + its plane buffers are compiled,
# so the two panels never both allocate RAM in one image.

fw-1680:
	cargo fw-1680
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-1680-release:
	cargo fw-1680-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-1680:
	cargo flash-1680

flash-1680-release:
	cargo fw-1680-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# Game on the SSD1680 panel (152x152, 1:1 — no border scaling).
fw-1680-game:
	cargo fw-1680-game
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-1680-game-release:
	cargo fw-1680-game-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-1680-game:
	cargo fw-1680-game
	probe-rs download --chip nRF52840_xxAA $(ELF)

fw-1680-mesh:
	cargo fw-1680-mesh
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-1680-mesh:
	cargo fw-1680-mesh
	probe-rs download --chip nRF52840_xxAA $(ELF)

# Organizer edition on the SSD1680 panel.
fw-1680-organizer:
	cargo fw-1680-organizer
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-1680-organizer-release:
	cargo fw-1680-organizer-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-1680-organizer:
	cargo fw-1680-organizer
	probe-rs download --chip nRF52840_xxAA $(ELF)

dfu-flash-1680:
	cargo fw-1680
	arm-none-eabi-objcopy -O binary $(ELF) $(BIN)
	dfu-util -w -D $(BIN)

dfu-flash-1680-release:
	cargo fw-1680-release
	arm-none-eabi-objcopy -O binary $(ELF_REL) $(BIN_REL)
	dfu-util -w -D $(BIN_REL)

flash-mesh-release:
	cargo fw-mesh-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# ---------- Organizer edition (calendar-first, no game) ----------

fw-organizer:
	cargo fw-organizer
	@arm-none-eabi-size $(ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

fw-organizer-release:
	cargo fw-organizer-release
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'

flash-organizer:
	cargo fw-organizer
	probe-rs download --chip nRF52840_xxAA $(ELF)

flash-organizer-release:
	cargo fw-organizer-release
	probe-rs download --chip nRF52840_xxAA $(ELF_REL)

# Build the organizer edition and push it over USB DFU (badge must be in
# bootloader mode).
dfu-flash-organizer: fw-organizer-release
	arm-none-eabi-objcopy -O binary $(ELF_REL) $(BIN_REL)
	dfu-util -w -D $(BIN_REL)

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

# Bundle hwtest artifacts (elf/bin/hex) for factory programmers (J-Link, etc.)
# Hwtest is standalone — flashes at 0x0, no bootloader needed.
fw-hwtest-bin: fw-hwtest
	@mkdir -p $(FW_OUT)
	cp $(ELF_HWTEST) $(FW_OUT)/cyber-aegg-hwtest.elf
	arm-none-eabi-objcopy -O binary $(ELF_HWTEST) $(FW_OUT)/cyber-aegg-hwtest.bin
	arm-none-eabi-objcopy -O ihex   $(ELF_HWTEST) $(FW_OUT)/cyber-aegg-hwtest.hex
	@arm-none-eabi-size $(ELF_HWTEST) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'
	@echo "hwtest ELF: $(abspath $(FW_OUT)/cyber-aegg-hwtest.elf)"
	@echo "hwtest BIN: $(abspath $(FW_OUT)/cyber-aegg-hwtest.bin) (load @ 0x00000000)"
	@echo "hwtest HEX: $(abspath $(FW_OUT)/cyber-aegg-hwtest.hex)"

# Full flash image — bootloader + app combined into one .hex/.bin for a
# single-shot SWD/J-Link reflash (recovery path for badges with very
# old bootloaders that can't be DFU'd).  Emits the individual pieces
# too so the user can pick what they need.
#
#  layout: 0x00000000 bootloader (~28 KB) → padded to 0x00010000 → app
fw-full-bin: bl-bin fw-bin-release
	@mkdir -p $(FW_OUT)
	arm-none-eabi-objcopy -O ihex   $(BL_ELF) $(FW_OUT)/nrf-aegg-bootloader.hex
	arm-none-eabi-objcopy -O ihex   $(ELF_REL) $(FW_OUT)/cyber-aegg.hex
	cp $(ELF_REL) $(FW_OUT)/cyber-aegg.elf
	# Combined HEX — strip BL's EOF marker, then concat app HEX (which
	# carries its own EOF + the app's "start linear address" record).
	grep -v '^:00000001FF' $(FW_OUT)/nrf-aegg-bootloader.hex > $(FW_OUT)/cyber-aegg-full.hex
	cat $(FW_OUT)/cyber-aegg.hex >> $(FW_OUT)/cyber-aegg-full.hex
	# Combined BIN — derived from the combined HEX so the two files
	# are byte-identical (gap regions are 0xFF in both).  Generating
	# the BIN by raw concat of the per-binary .bin files would emit
	# 0x00 in section gaps that objcopy -O binary leaves un-filled,
	# which mismatches the HEX and confuses verify-after-program.
	arm-none-eabi-objcopy -I ihex -O binary --gap-fill=0xff \
	    $(FW_OUT)/cyber-aegg-full.hex $(FW_OUT)/cyber-aegg-full.bin
	@echo
	@echo "full HEX: $(abspath $(FW_OUT)/cyber-aegg-full.hex)  (load via J-Link / nrfjprog / probe-rs)"
	@echo "full BIN: $(abspath $(FW_OUT)/cyber-aegg-full.bin)  (load @ 0x00000000)"
	@echo "bl ELF:   $(abspath $(FW_OUT)/nrf-aegg-bootloader.elf)"
	@echo "app ELF:  $(abspath $(FW_OUT)/cyber-aegg.elf)"

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

# Build the bootloader release artifact for the SWD mass-programmer
# (../mass-flash-bl). probe-rs flashes the ELF directly (it
# carries its own load addresses), so the ELF is what mass-flash-bl consumes;
# a raw .bin is also emitted for convenience, plus a .hex for the Nordic tools.
bl-bin:
	cd bootloader && cargo bl
	@mkdir -p $(FW_OUT)
	cp $(BL_ELF) $(FW_OUT)/nrf-aegg-bootloader.elf
	arm-none-eabi-objcopy -O binary $(BL_ELF) $(FW_OUT)/nrf-aegg-bootloader.bin
	arm-none-eabi-objcopy -O ihex   $(BL_ELF) $(FW_OUT)/nrf-aegg-bootloader.hex
	@arm-none-eabi-size $(BL_ELF) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'
	@echo "bootloader ELF (pass to mass-flash-bl -f): $(abspath $(FW_OUT)/nrf-aegg-bootloader.elf)"
	@echo "raw bin (optional):                        $(abspath $(FW_OUT)/nrf-aegg-bootloader.bin)"
	@echo "hex (nrfjprog / nRF Connect Programmer):   $(abspath $(FW_OUT)/nrf-aegg-bootloader.hex)"

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

# Build the release .bin and drop it in the auto-flasher's firmware dir so
# cyber-aegg-flasher serves it for WebUSB DFU. Does not flash anything.
# The .hex comes from the ELF, not the .bin: `-O binary` discards the load
# address, and the Nordic tools place records by address (app sits at
# 0x00010000, above the bootloader).
fw-bin-release:
	cargo fw-release
	@mkdir -p $(FW_OUT)
	arm-none-eabi-objcopy -O binary $(ELF_REL) $(FW_OUT)/cyber-aegg.bin
	arm-none-eabi-objcopy -O ihex   $(ELF_REL) $(FW_OUT)/cyber-aegg.hex
	@arm-none-eabi-size $(ELF_REL) | tail -1 | awk '{printf "  flash: %s B  ram: %s B\n", $$1+$$2, $$3}'
	@echo "app bin (pass to the DFU flasher):       $(abspath $(FW_OUT)/cyber-aegg.bin)"
	@echo "app hex (nrfjprog / nRF Connect Prog.):  $(abspath $(FW_OUT)/cyber-aegg.hex)"
