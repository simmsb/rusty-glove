MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */

  /* FLASH is where the bootloader is placed */
  FLASH                             : ORIGIN = 0x00000000, LENGTH = 24K
  BOOTLOADER_STATE                  : ORIGIN = ORIGIN(FLASH) + LENGTH(FLASH), LENGTH = 4K
  ACTIVE                            : ORIGIN = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE), LENGTH = 472K
  CONFIG                            : ORIGIN = ORIGIN(ACTIVE) + LENGTH(ACTIVE), LENGTH = 48K
  /* DFU must be one page larger than ACTIVE */
  DFU                               : ORIGIN = ORIGIN(CONFIG) + LENGTH(CONFIG), LENGTH = 476K
  RAM                         (rwx) : ORIGIN = 0x20000000, LENGTH = 256K
}

__bootloader_state_start = ORIGIN(BOOTLOADER_STATE);
__bootloader_state_end = ORIGIN(BOOTLOADER_STATE) + LENGTH(BOOTLOADER_STATE);

__bootloader_active_start = ORIGIN(ACTIVE);
__bootloader_active_end = ORIGIN(ACTIVE) + LENGTH(ACTIVE);

__bootloader_dfu_start = ORIGIN(DFU);
__bootloader_dfu_end = ORIGIN(DFU) + LENGTH(DFU);
