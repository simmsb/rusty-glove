//! Forwards a TCP port on the USB side to a port on one of the halves
//!
//! The stack can't route between its interfaces, so this is how the host
//! reaches the halves.

use embassy_futures::join::join;
use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    Stack,
};
use embassy_time::{with_timeout, Duration};
use embedded_io_async::{Read, Write};
use shared::net::{port, usb as usb_net, Node};

/// Give up on either end if it goes quiet for this long, a bit longer than
/// the half waits so it's the one that gives up
const TIMEOUT: Duration = Duration::from_secs(75);
const BUF_LEN: usize = 4096;

/// Copy from `r` to `w` until `r` closes or either fails
async fn pump(mut r: impl Read, mut w: impl Write) {
    let mut buf = [0u8; 512];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if w.write_all(&buf[..n]).await.is_err() {
                    return;
                }
            }
        }
    }
    _ = w.flush().await;
}

#[embassy_executor::task(pool_size = 2)]
pub async fn proxy_task(stack: Stack<'static>, listen_port: u16, target: Node) -> ! {
    let mut listener = TcpListener::new(stack).unwrap();
    listener.listen((usb_net::DONGLE_IP, listen_port)).unwrap();

    let mut host_rx = [0u8; BUF_LEN];
    let mut host_tx = [0u8; BUF_LEN];
    let mut target_rx = [0u8; BUF_LEN];
    let mut target_tx = [0u8; BUF_LEN];

    loop {
        let Ok(token) = listener.accept().await else {
            continue;
        };

        let mut host = TcpSocket::new(stack, &mut host_rx, &mut host_tx).unwrap();
        host.set_timeout(Some(TIMEOUT));
        if host.accept(token).await.is_err() {
            continue;
        }

        let mut upstream = TcpSocket::new(stack, &mut target_rx, &mut target_tx).unwrap();
        upstream.set_timeout(Some(TIMEOUT));

        defmt::info!("Forwarding port {} to {}", listen_port, target);

        match upstream.connect((target.link_local(), port::DFU)).await {
            Ok(()) => {
                let (host_r, host_w) = host.split();
                let (up_r, up_w) = upstream.split();

                // each direction ends when its sender closes, which for the
                // DFU protocol is the half after it answers, then the host
                join(pump(host_r, up_w), pump(up_r, host_w)).await;

                upstream.close();
                _ = with_timeout(Duration::from_secs(1), upstream.flush()).await;
            }
            Err(e) => defmt::warn!("Couldn't reach {}: {}", target, e),
        }

        host.close();
        _ = with_timeout(Duration::from_secs(1), host.flush()).await;
        host.abort();
        upstream.abort();
    }
}
