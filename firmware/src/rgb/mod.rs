use embassy_executor::Spawner;
use embassy_nrf::{gpio::AnyPin, peripherals::PWM0};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};

use crate::{
    interboard::{self, THIS_SIDE_MESSAGE_BUS},
    messages::device_to_device::DeviceToDevice,
    side,
};

use self::{animation::Animation, animations::DynAnimation};

pub mod animation;
pub mod animations;
mod driver;
pub mod layout;
pub mod math_utils;
mod runner;

pub(super) static RGB_CMD_CHANNEL: Channel<ThreadModeRawMutex, Command, 1> = Channel::new();

pub fn init(spawner: &Spawner, pwm: PWM0, pin: AnyPin) {
    let d = driver::Ws2812::new(pwm, pin);

    spawner.must_spawn(runner::rgb_runner(d));
    spawner.must_spawn(runner::apply_keypresses());
    spawner.must_spawn(runner::sparkle_ticker());
    spawner.must_spawn(command_listener());

    if side::is_master() {
        spawner.must_spawn(animation_randomizer());
    }
}

#[embassy_executor::task]
async fn command_listener() {
    let mut sub = THIS_SIDE_MESSAGE_BUS.subscriber().unwrap();

    loop {
        let cmd = match sub.next_message_pure().await {
            DeviceToDevice::SetAnimation(a) => Command::SetNextAnimation(a),
            DeviceToDevice::SyncAnimation(a) => Command::SyncAnimation(a),
            _ => continue,
        };

        send_cmd(cmd).await;
    }
}

#[embassy_executor::task]
async fn animation_randomizer() {
    loop {
        Timer::after(Duration::from_secs(60 * 5)).await;

        let anim = DynAnimation::random();
        let sync = anim.construct_sync();

        send_cmd(Command::SetNextAnimation(sync.clone())).await;
        interboard::send_msg(DeviceToDevice::SetAnimation(sync), 3).await;
    }
}

pub async fn send_cmd(cmd: Command) {
    RGB_CMD_CHANNEL.send(cmd).await
}

pub enum Command {
    SetNextAnimation(animations::AnimationSync),
    SyncAnimation(animations::AnimationSync),
}
