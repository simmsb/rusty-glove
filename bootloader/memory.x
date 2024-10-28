MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */

  MBR                               : ORIGIN = 0x00000000, LENGTH = 4K
  SOFTDEVICE                        : ORIGIN = 0x00001000, LENGTH = 152K
  /* NRF52840 with Softdevice S140 7.3.0 */

  ACTIVE                            : ORIGIN = ORIGIN(SOFTDEVICE) + LENGTH(SOFTDEVICE), LENGTH = 316K
  CONFIG                            : ORIGIN = ORIGIN(ACTIVE) + LENGTH(ACTIVE), LENGTH = 48K
  DFU                               : ORIGIN = ORIGIN(CONFIG) + LENGTH(CONFIG), LENGTH = 320K
  /* FLASH is where the bootloader is placed */
  FLASH                             : ORIGIN = ORIGIN(DFU) + LENGTH(DFU), LENGTH = 24K
  BOOTLOADER_STATE                  : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 4K
  RAM                         (rwx) : ORIGIN = 0x20000008, LENGTH = 0x2fff8
  uicr_bootloader_start_address (r) : ORIGIN = 0x10001014, LENGTH = 0x4
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE);

__bootloader_active_start = ORIGIN(ACTIVE);
__bootloader_active_end = ORIGIN(ACTIVE) + LENGTH(ACTIVE);

__bootloader_dfu_start = ORIGIN(DFU);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU);

__bootloader_start = ORIGIN(FLASH);

SECTIONS
{
  .uicr_bootloader_start_address :
  {
    LONG(__bootloader_start)
  } > uicr_bootloader_start_address
}
