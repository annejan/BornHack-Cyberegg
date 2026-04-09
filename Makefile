ELF         = target/thumbv7em-none-eabihf/debug/embassy
BIN         = target/thumbv7em-none-eabihf/debug/embassy.bin
BIN_REL     = target/thumbv7em-none-eabihf/release/embassy.bin
ELF_REL     = target/thumbv7em-none-eabihf/release/embassy

# App flash base matches the app slot in memory.x (ORIGIN in memory.x = 0xD000)
FLASH_BASE = 0x0000D000

.PHONY: fw fw-release fw-game fw-game-release fw-mesh fw-mesh-release \
        sim flash flash-release flash-game flash-mesh \
        monitor bl bl-flash dfu-flash dfu-flash-release

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

# ---------- Simulator ----------

sim:
	cargo sim

# ---------- Monitor / debug ----------

monitor:
	probe-rs attach --chip nRF52840_xxAA --always-print-stacktrace target/thumbv7em-none-eabihf/debug/embassy

bl-monitor:
	probe-rs attach --chip nRF52840_xxAA bootloader/target/thumbv7em-none-eabihf/release/nrf-aegg-bootloader

# ---------- Bootloader ----------

bl:
	cd bootloader && cargo bl

bl-flash:
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
