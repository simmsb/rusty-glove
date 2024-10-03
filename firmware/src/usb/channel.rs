use embassy_executor::Spawner;
use embassy_sync::channel::Channel;
use embassy_sync::pipe::{Pipe, Reader, Writer};
use embassy_sync::pubsub::PubSubChannel;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State};
use embassy_usb::driver::Driver;
use embassy_usb::Builder;
use shared::device_to_host::DeviceToHost;
use shared::host_to_device::HostToDevice;

use crate::messages::transmissions;
use crate::utils;

use super::{USBDriver, MAX_PACKET_SIZE};

pub static COMMANDS_FROM_HOST: PubSubChannel<CS, HostToDevice, 4, 4, 1> = PubSubChannel::new();
pub static COMMANDS_TO_HOST: Channel<CS, DeviceToHost, 16> = Channel::new();

const BUF_SIZE: usize = 128;

#[embassy_executor::task]
async fn serial_in_task(
    out_pipe: Writer<'static, CS, BUF_SIZE>,
    mut serial_rx: Receiver<'static, USBDriver>,
) {
    loop {
        let mut rx: [u8; MAX_PACKET_SIZE as usize] = [0; MAX_PACKET_SIZE as usize];
        serial_rx.wait_connection().await;
        while let Ok(len) = serial_rx.read_packet(&mut rx[..]).await {
            let _ = out_pipe.write(&rx[..len]).await;
        }
    }
}

#[embassy_executor::task]
async fn serial_out_task(
    in_pipe: Reader<'static, CS, BUF_SIZE>,
    mut serial_tx: Sender<'static, USBDriver>,
) {
    loop {
        let mut rx: [u8; MAX_PACKET_SIZE as usize] = [0; MAX_PACKET_SIZE as usize];
        serial_tx.wait_connection().await;
        loop {
            let len = in_pipe.read(&mut rx[..]).await;
            if serial_tx.write_packet(&rx[..len]).await.is_err() {
                break;
            }
        }
    }
}

#[embassy_executor::task]
async fn eventer_task(tx: Writer<'static, CS, BUF_SIZE>, rx: Reader<'static, CS, BUF_SIZE>) {
    let msg_pub = COMMANDS_FROM_HOST.publisher().unwrap();
    let rx_fn = || async { (COMMANDS_TO_HOST.receive().await, None) };
    let tx_fn = |e| async {
        msg_pub.publish(e).await;
    };
    transmissions::eventer(tx, rx, rx_fn, tx_fn).await;
}

pub fn init(spawner: &Spawner, builder: &mut Builder<'static, USBDriver>) {
    let cdc_state = utils::singleton!(State, State::new());
    let from_usb_pipe = utils::singleton!(Pipe<CS, BUF_SIZE>, Pipe::new());
    let to_usb_pipe = utils::singleton!(Pipe<CS, BUF_SIZE>, Pipe::new());

    let state = make_state(cdc_state, builder);
    let (serial_tx, serial_rx) = state.class.split();
    let (from_usb_rx, from_usb_tx) = from_usb_pipe.split();
    let (to_usb_rx, to_usb_tx) = to_usb_pipe.split();

    spawner.must_spawn(serial_in_task(from_usb_tx, serial_rx));
    spawner.must_spawn(serial_out_task(to_usb_rx, serial_tx));
    spawner.must_spawn(eventer_task(to_usb_tx, from_usb_rx));
}

pub fn make_state<'d, D>(
    cdc_state: &'d mut State<'d>,
    builder: &mut Builder<'d, D>,
) -> SerialState<'d, D>
where
    D: Driver<'d>,
{
    // Create classes on the builder.
    let class = CdcAcmClass::new(builder, cdc_state, MAX_PACKET_SIZE);

    SerialState { class }
}

type CS = embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

pub struct SerialState<'d, D: Driver<'d>> {
    class: CdcAcmClass<'d, D>,
}
