MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  /* NRF52840 with Softdevice S140 7.3.0 */
  FLASH : ORIGIN = 0x00000000 + 156K, LENGTH = 512K - (156K + 8K)
  CONFIG : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 8K
  RAM : ORIGIN = 0x20000000 + 0x10190, LENGTH = 256K - 0x10190
}

__config_start = ORIGIN(CONFIG) - ORIGIN(FLASH);
__config_end = __config_start + LENGTH(CONFIG);

SECTIONS {
  .config (NOLOAD) : ALIGN(4)
  {
    . = ALIGN(4);
    *(.config .config.*);
    . = ALIGN(4);
  } > CONFIG
}
