MEMORY
{
  /* nRF52840 — 1 MB internal flash, 256 KB RAM                */
  /* Bootloader occupies the first 64 K of flash.              */
  /* App occupies the remaining ~960 K (0x10000 – 0x0FFFFF).   */
  FLASH (rx)  : ORIGIN = 0x00000000, LENGTH = 64K
  RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 256K
}

/* Exported so the bootloader can validate and jump to the app. */
APP_START = 0x00010000;
