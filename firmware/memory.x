
MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */

  MBR                               : ORIGIN = 0x00000000, LENGTH = 4K
  SOFTDEVICE                        : ORIGIN = 0x00001000, LENGTH = 152K
  /* NRF52840 with Softdevice S140 7.3.0 */
  FLASH                             : ORIGIN = ORIGIN(SOFTDEVICE) + LENGTH(SOFTDEVICE), LENGTH = 316K
  CONFIG                            : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 48K
  DFU                               : ORIGIN = ORIGIN(CONFIG) + LENGTH(CONFIG), LENGTH = 320K
  BOOTLOADER                        : ORIGIN = ORIGIN(DFU) + LENGTH(DFU), LENGTH = 24K
  BOOTLOADER_STATE                  : ORIGIN = ORIGIN(BOOTLOADER) + LENGTH(BOOTLOADER), LENGTH = 4K
  RAM                         (rwx) : ORIGIN = 0x20000000 + 0x10318, LENGTH = 256K - 0x10318
  uicr_bootloader_start_address (r) : ORIGIN = 0x10001014, LENGTH = 0x4
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
