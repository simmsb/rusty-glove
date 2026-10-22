use embassy_executor::Spawner;
use embassy_nrf::usb::{vbus_detect::HardwareVbusDetect, Driver};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_usb::{
    class::{
        cdc_ncm::device::{self as ncm, embassy_net as ncm_net, CdcNcmClass},
        hid::device::{HidWriter, State},
    },
    Builder, Config,
};
use portable_atomic::{AtomicU8, Ordering};
use shared::net::{usb as usb_net, HostOs, KEYBOARD_REPORT_LEN};
use usbd_human_interface_device::device::keyboard::NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR;

use crate::singleton;

const MAX_PACKET_SIZE: u16 = 64;
const ETHERNET_MTU: usize = 1514;

/// Packed reports from the keyboard, waiting to go to the host
pub static KEYBOARD_REPORTS: Channel<ThreadModeRawMutex, [u8; KEYBOARD_REPORT_LEN], 8> =
    Channel::new();

const OS_UNGUESSED: u8 = u8::MAX;
static GUESSED_OS: AtomicU8 = AtomicU8::new(OS_UNGUESSED);

pub fn guessed_host_os() -> Option<HostOs> {
    Some(match GUESSED_OS.load(Ordering::Relaxed) {
        0 => HostOs::Windows,
        1 => HostOs::Linux,
        2 => HostOs::MacOS,
        3 => HostOs::Unknown,
        _ => return None,
    })
}

mod usb_driver {
    use super::{Ordering, GUESSED_OS};
    use embassy_nrf::usb::{vbus_detect::HardwareVbusDetect, Driver};
    use embassy_os_guess::OS;

    pub type USBDriver = impl embassy_usb::driver::Driver<'static>;

    #[define_opaque(USBDriver)]
    pub fn set_guesser(driver: Driver<'static, HardwareVbusDetect>) -> USBDriver {
        let guesser = embassy_os_guess::OSGuesser::new(|guess| {
            defmt::info!("Guessed host os: {}", guess);

            let v = match guess {
                OS::Windows => 0,
                OS::Linux => 1,
                OS::MacOS => 2,
                OS::Unknown => 3,
            };
            GUESSED_OS.store(v, Ordering::Relaxed);
        });
        guesser.wrap_driver(driver)
    }
}

use usb_driver::{set_guesser, USBDriver};

pub type EthernetDevice = ncm_net::Device<'static>;

#[embassy_executor::task]
async fn run_usb(builder: Builder<'static, USBDriver>) {
    let mut device = builder.build();
    device.run().await;
}

#[embassy_executor::task]
async fn keyboard_writer(mut writer: HidWriter<'static, USBDriver, 64>) {
    loop {
        let report = KEYBOARD_REPORTS.receive().await;
        if let Err(e) = writer.write(&report).await {
            defmt::trace!("Failed to write report: {}", e);
        }
    }
}

#[embassy_executor::task]
async fn ethernet_task(runner: ncm_net::Runner<'static, USBDriver>) -> ! {
    runner.run().await
}

/// Sets up USB as a keyboard and an ethernet adapter, returning the ethernet
/// side for the network stack
pub fn init(spawner: &Spawner, driver: Driver<'static, HardwareVbusDetect>) -> EthernetDevice {
    let driver = set_guesser(driver);

    // a composite device, the default class/subclass/protocol are the ones
    // that go with interface association descriptors
    let mut config = Config::new(0x1209, 0x0001);
    config.manufacturer = Some("Ben Simms");
    config.product = Some("Glove80 Dongle");
    config.serial_number = Some("0001");
    config.max_power = 100;
    config.max_packet_size_0 = MAX_PACKET_SIZE as u8;

    let mut builder = Builder::new(
        driver,
        config,
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
    );

    let writer = HidWriter::new(
        &mut builder,
        singleton!(State, State::new()),
        embassy_usb::class::hid::device::Config {
            report_descriptor: NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR,
            request_handler: None,
            poll_ms: 1,
            max_packet_size: 64,
            hid_subclass: embassy_usb::class::hid::HidSubclass::No,
            hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
        },
    );

    let ncm = CdcNcmClass::new(
        &mut builder,
        singleton!(ncm::State<'static>, ncm::State::new()),
        usb_net::HOST_MAC,
        MAX_PACKET_SIZE,
    );
    let (ethernet_runner, ethernet) = ncm.into_embassy_net_device::<4, 4>(
        singleton!(ncm_net::State<4, 4>, ncm_net::State::new()),
        usb_net::DONGLE_MAC,
        ETHERNET_MTU,
    );

    spawner.spawn(keyboard_writer(writer).unwrap());
    spawner.spawn(ethernet_task(ethernet_runner).unwrap());
    spawner.spawn(run_usb(builder).unwrap());

    ethernet
}
