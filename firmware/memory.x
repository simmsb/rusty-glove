MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */

  BOOTLOADER                        : ORIGIN = 0x00000000, LENGTH = 24K
  BOOTLOADER_STATE                  : ORIGIN = ORIGIN(BOOTLOADER) + LENGTH(BOOTLOADER), LENGTH = 4K
  FLASH                             : ORIGIN = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE), LENGTH = 472K
  CONFIG                            : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 48K
  /* DFU must be one page larger than FLASH */
  DFU                               : ORIGIN = ORIGIN(CONFIG) + LENGTH(CONFIG), LENGTH = 476K
  RAM                         (rwx) : ORIGIN = 0x20000000, LENGTH = 256K
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE);

__bootloader_dfu_start = ORIGIN(DFU);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU);

__config_start = ORIGIN(CONFIG);
__config_end = __config_start + LENGTH(CONFIG);

SECTIONS {
  .config (NOLOAD) : ALIGN(4)
  {
    . = ALIGN(4);
    *(.config .config.*);
    . = ALIGN(4);
  } > CONFIG
}
