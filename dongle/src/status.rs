//! A status page for seeing how the dongle is doing, on our USB side, see
//! [`glove_net::status`]

use core::fmt::Write;

use embassy_net::Stack;
use embassy_time::Instant;
use shared::net::{port, usb as usb_net};

#[embassy_executor::task]
pub async fn status_task(stack: Stack<'static>) -> ! {
    glove_net::status::serve::<1024>(stack, (usb_net::DONGLE_IP, port::STATUS), |page| {
        _ = render(page);
    })
    .await
}

fn render(page: &mut impl Write) -> core::fmt::Result {
    writeln!(page, "dongle, up {}s", Instant::now().as_secs())?;
    writeln!(page, "host os: {:?}", crate::usb::guessed_host_os())?;
    writeln!(page, "queued: reports {}", crate::usb::KEYBOARD_REPORTS.len())?;
    writeln!(page, "keyboard link: {:?}", crate::keyboard::LINK_STATS.read())?;
    writeln!(page, "radio: {:?}", glove_net::radio::stats())?;
    writeln!(page, "packet buffers ran out: {}", crate::net::pool_exhausted())?;
    Ok(())
}
