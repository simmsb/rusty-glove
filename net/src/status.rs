//! A plain text status page over TCP: connect, and it sends the page and
//! hangs up. For seeing how a node is doing without a probe attached.

use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    wire::ListenSocketAddr,
    Stack,
};
use embassy_time::{with_timeout, Duration};
use embedded_io_async::Write;

/// Serve the page `render` writes, it's cut short if it doesn't fit `N` bytes
pub async fn serve<const N: usize>(
    stack: Stack<'_>,
    local: impl Into<ListenSocketAddr>,
    mut render: impl FnMut(&mut heapless::String<N>),
) -> ! {
    let mut listener = TcpListener::new(stack).unwrap();
    listener.listen(local).unwrap();

    let mut rx_buf = [0u8; 64];
    let mut tx_buf = [0u8; 512];

    loop {
        let Ok(token) = listener.accept().await else {
            continue;
        };

        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        socket.set_timeout(Some(Duration::from_secs(10)));
        if socket.accept(token).await.is_ok() {
            let mut page = heapless::String::new();
            render(&mut page);
            _ = socket.write_all(page.as_bytes()).await;
            socket.close();
            _ = with_timeout(Duration::from_secs(5), socket.flush()).await;
        }
        socket.abort();
    }
}
