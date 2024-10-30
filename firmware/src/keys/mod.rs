use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_nrf::gpio::{Input, Output};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex, channel::Channel, pubsub::PubSubChannel,
};
use embassy_time::{Duration, Timer};
use keyberon::{key_code::KeyCode, layout::Event};
use packed_struct::PrimitiveEnum;
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardReport;

use crate::{
    interboard::{self},
    messages::device_to_device::DeviceToDevice,
    side,
    ble::hid::publish_keyboard_report,
    utils::Ticker,
};

use self::{chord::ChordingEngine, layout::LAYERS};

#[derive(Clone, Copy)]
pub enum UnicodeMode {
    Linux,
    Mac,
}

#[derive(Clone, Copy)]
pub enum CustomEvent {
    TypeUnicode(&'static str),
}

pub mod chord;
pub mod layout;
pub mod scan;
mod unicode;

/// Raw matrix presses and releases
pub static MATRIX_EVENTS: Channel<ThreadModeRawMutex, keyberon::layout::Event, 4> =
    Channel::new();

/// Chord-processed events
pub static KEY_EVENTS: PubSubChannel<ThreadModeRawMutex, keyberon::layout::Event, 4, 4, 2> =
    PubSubChannel::new();

pub type ScannerInstance<'a> = scan::Scanner<
    (
        Output<'a>,
        Output<'a>,
        Output<'a>,
        Output<'a>,
        Output<'a>,
        Output<'a>,
        Output<'a>,
    ),
    (
        Input<'a>,
        Input<'a>,
        Input<'a>,
        Input<'a>,
        Input<'a>,
        Input<'a>,
    ),
>;

#[embassy_executor::task]
async fn matrix_scanner(mut scanner: ScannerInstance<'static>) {
    // TODO: pause when no activity

    let matrix_events = MATRIX_EVENTS.sender();

    loop {
        for evt in scanner.scan() {
            matrix_events.send(evt).await;
        }

        // use a timer instead of a ticker here, prevents getting stuck if matrix_events freezes
        Timer::after(Duration::from_hz(2000)).await;
    }
}

#[embassy_executor::task]
async fn matrix_processor() {
    let sub = MATRIX_EVENTS.receiver();
    let key_events = KEY_EVENTS.publisher().unwrap();
    let mut chorder = ChordingEngine::new(layout::chorder());
    let mut ticker = Ticker::every(Duration::from_hz(1000));

    loop {
        match select(ticker.next(), sub.receive()).await {
            embassy_futures::select::Either::Second(evt) => {
                //key_events.publish(evt).await;
                let evts = chorder.process(evt);
                for evt in evts {
                    embassy_futures::join::join(
                        key_events.publish(evt),
                        send_to_other_side(evt),
                    )
                    .await;
                }
            }
            embassy_futures::select::Either::First(_) => {
                let keys = chorder.tick();
                for (x, y) in keys {
                    let evt = keyberon::layout::Event::Press(x, y);
                    embassy_futures::join::join(
                        key_events.publish(evt),
                        send_to_other_side(evt),
                    )
                    .await;
                }
            }
        }
    }
}

async fn send_to_other_side(evt: Event) {
    let evt = match evt {
        Event::Press(x, y) => DeviceToDevice::KeyPress(x, y),
        Event::Release(x, y) => DeviceToDevice::KeyRelease(x, y),
    };
    interboard::send_msg(evt, 1).await;
}

#[embassy_executor::task]
async fn receive_events_from_other_side() {
    let mut sub = crate::interboard::THIS_SIDE_MESSAGE_BUS
        .subscriber()
        .unwrap();
    let key_events = KEY_EVENTS.publisher().unwrap();

    loop {
        let evt = match sub.next_message_pure().await {
            DeviceToDevice::KeyPress(x, y) => Event::Press(x, y),
            DeviceToDevice::KeyRelease(x, y) => Event::Release(x, y),
            _ => {
                continue;
            }
        };

        key_events.publish(evt).await;
    }
}

#[embassy_executor::task]
async fn key_event_processor() {
    let mut sub = KEY_EVENTS.subscriber().unwrap();
    let mut layout = keyberon::layout::Layout::new(&LAYERS);
    let mut state = heapless::Vec::<KeyCode, 24>::new();
    let mut ticker = Ticker::every(Duration::from_hz(1000));

    loop {
        match select(ticker.next(), sub.next_message_pure()).await {
            embassy_futures::select::Either::Second(evt) => {
                // crate::utils::log::info!("evt: {:?}", evt);

                layout.event(evt);
            }
            embassy_futures::select::Either::First(_) => {
                let cevent = layout.tick();
                if let Some((evt, is_press)) = match cevent {
                    keyberon::layout::CustomEvent::NoEvent => None,
                    keyberon::layout::CustomEvent::Press(m) => Some((*m, true)),
                    keyberon::layout::CustomEvent::Release(m) => Some((*m, false)),
                } {
                    match evt {
                        CustomEvent::TypeUnicode(msg) => {
                            if !is_press {
                                unicode::send_unicode(msg).await;
                            }
                        }
                    }
                }
            }
        }

        let new_state = heapless::Vec::<_, 24>::from_iter(layout.keycodes());

        if new_state != state {
            state = new_state;

            publish_keyboard_report(NKROBootKeyboardReport::new(state.iter().filter_map(|k| {
                usbd_human_interface_device::page::Keyboard::from_primitive(*k as u8)
            })))
            .await;
        }
    }
}

pub fn init(spawner: &Spawner, scanner: ScannerInstance<'static>) {
    spawner.must_spawn(matrix_processor());
    spawner.must_spawn(matrix_scanner(scanner));
    spawner.must_spawn(receive_events_from_other_side());
    if side::is_master() {
        spawner.must_spawn(key_event_processor());
        spawner.must_spawn(unicode::unicode_task());
    }
}
