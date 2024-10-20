#![no_std]
#![no_main]

use core::cell::RefCell;

use cortex_m_rt::{entry, exception};
use defmt_rtt as _;
use embassy_boot_nrf::*;
use embassy_embedded_hal::flash::partition::BlockingPartition;
use embassy_nrf::nvmc::Nvmc;
use embassy_nrf::wdt;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;
#[cfg(feature = "panic-probe")]
use panic_probe as _;

#[entry]
fn main() -> ! {
    let p = embassy_nrf::init(Default::default());

    // Uncomment this if you are debugging the bootloader with debugger/RTT attached,
    // as it prevents a hard fault when accessing flash 'too early' after boot.
    for _i in 0..10000000 {
        cortex_m::asm::nop();
    }

    let mut wdt_config = wdt::Config::default();
    wdt_config.timeout_ticks = 32768 * 20; // timeout seconds
    wdt_config.run_during_sleep = true;
    wdt_config.run_during_debug_halt = false;

    let internal_flash = WatchdogFlash::start(Nvmc::new(p.NVMC), p.WDT, wdt_config);
    let internal_flash = Mutex::new(RefCell::new(internal_flash));

    let active_offset;
    let bl: BootLoader = {
        let config = create_flash_config(&internal_flash);
        active_offset = config.active.offset();

        BootLoader::prepare(config)
    };

    unsafe { bl.load(active_offset) }
}

pub fn create_flash_config<'a, 'b, 'c>(
    internal: &'a Mutex<NoopRawMutex, RefCell<WatchdogFlash<Nvmc<'b>>>>,
) -> BootLoaderConfig<
    BlockingPartition<'a, NoopRawMutex, WatchdogFlash<Nvmc<'b>>>,
    BlockingPartition<'a, NoopRawMutex, WatchdogFlash<Nvmc<'b>>>,
    BlockingPartition<'a, NoopRawMutex, WatchdogFlash<Nvmc<'b>>>,
> {
    extern "C" {
        static __bootloader_state_start: u32;
        static __bootloader_state_end: u32;
        static __bootloader_active_start: u32;
        static __bootloader_active_end: u32;
        static __bootloader_dfu_start: u32;
        static __bootloader_dfu_end: u32;
    }

    let active = unsafe {
        let start = &__bootloader_active_start as *const u32 as u32;
        let end = &__bootloader_active_end as *const u32 as u32;
        // trace!("ACTIVE: 0x{:x} - 0x{:x}", start, end);

        BlockingPartition::new(internal, start, end - start)
    };
    let dfu = unsafe {
        let start = &__bootloader_dfu_start as *const u32 as u32;
        let end = &__bootloader_dfu_end as *const u32 as u32;
        // trace!("DFU: 0x{:x} - 0x{:x}", start, end);

        BlockingPartition::new(internal, start, end - start)
    };
    let state = unsafe {
        let start = &__bootloader_state_start as *const u32 as u32;
        let end = &__bootloader_state_end as *const u32 as u32;
        // trace!("STATE: 0x{:x} - 0x{:x}", start, end);

        BlockingPartition::new(internal, start, end - start)
    };

    BootLoaderConfig { active, dfu, state }
}

#[no_mangle]
#[cfg_attr(target_os = "none", link_section = ".HardFault.user")]
unsafe extern "C" fn HardFault() {
    cortex_m::peripheral::SCB::sys_reset();
}

#[exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    const SCB_ICSR: *const u32 = 0xE000_ED04 as *const u32;
    let irqn = core::ptr::read_volatile(SCB_ICSR) as u8 as i16 - 16;

    panic!("DefaultHandler #{:?}", irqn);
}

#[cfg(not(feature = "panic-probe"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::asm::udf();
}
