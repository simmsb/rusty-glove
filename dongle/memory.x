MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */

  /* bare nRF52840 flashed over SWD, no bootloader */
  FLASH                             : ORIGIN = 0x00000000, LENGTH = 1024K
  RAM                         (rwx) : ORIGIN = 0x20000000, LENGTH = 256K
}
