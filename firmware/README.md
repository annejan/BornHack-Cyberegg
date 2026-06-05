# firmware/ — built flasher artifacts

`make` writes the host-flasher artifacts here. They are git-ignored; build them
locally, then point the flasher tools at the printed path (no directory coupling
between this firmware project and the flashers).

| Make target       | Output here                     | Used by |
|-------------------|---------------------------------|---------|
| `make bl-bin`     | `nrf-aegg-bootloader.elf` (+`.bin`) | SWD `mass-flash-bl -f <path>` |
| `make fw-bin-release` | `cyber-aegg.bin`            | USB DFU `cyber-aegg-flasher-cli` |

Each target prints the absolute path of its artifact when it finishes — copy
that path into the flasher invocation, e.g.:

```sh
make bl-bin
mass-flash-bl -f /abs/path/firmware/nrf-aegg-bootloader.elf

make fw-bin-release
cyber-aegg-flasher-cli watch /abs/path/firmware/cyber-aegg.bin --auto-copy --payload-dir ...
```
