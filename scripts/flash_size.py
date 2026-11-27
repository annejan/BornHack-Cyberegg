#!/usr/bin/env python3
"""Flat flash image size of a firmware ELF, gated at a limit.

Makes the same flat binary the Makefile's dfu-flash targets make (there with
arm-none-eabi-objcopy; here with the rustup llvm-objcopy, so CI needs only the
`llvm-tools` component). Prints the size, warns above 940 KiB and exits 1 above the limit.
"""
import argparse
import os
import subprocess
import sys
import tempfile

BYTES_PER_KIB = 1024
# Print a warning from here up to the hard limit.
WARN_KIB = 940


def llvm_objcopy():
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    host = next(
        line.split(": ", 1)[1]
        for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines()
        if line.startswith("host: ")
    )
    return os.path.join(sysroot, "lib", "rustlib", host, "bin", "llvm-objcopy")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("elf", help="firmware ELF, e.g. target/thumbv7em-none-eabihf/release/embassy")
    ap.add_argument("--limit-kib", type=int, required=True, help="largest allowed image, in KiB")
    args = ap.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        image = os.path.join(tmp, "image.bin")
        subprocess.check_call([llvm_objcopy(), "-O", "binary", args.elf, image])
        size = os.path.getsize(image)
    kib = size / BYTES_PER_KIB
    print(f"flash image: {size} B ({kib:.1f} KiB of {args.limit_kib} KiB)")
    if size > args.limit_kib * BYTES_PER_KIB:
        print("flash image is over the limit", file=sys.stderr)
        return 1
    if kib > WARN_KIB:
        print(f"warning: flash image is above {WARN_KIB} KiB, close to the {args.limit_kib} KiB limit")
    return 0


if __name__ == "__main__":
    sys.exit(main())
