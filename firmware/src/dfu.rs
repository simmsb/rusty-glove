//! Firmware updates over the network, the protocol is described in
//! [`shared::net::dfu`]

use embassy_boot::{AlignedBuffer, FirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_boot_nrf::FirmwareState;
use embassy_embedded_hal::flash::partition::Partition;
use embassy_executor::Spawner;
use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    Stack,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::{with_timeout, Duration, Timer};
use portable_atomic::{AtomicBool, Ordering};
use embedded_io_async::{Read, Write};
use embedded_storage::nor_flash::ErrorType;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use shared::net::{
    dfu::{Status, MAGIC, OP_CONFIRM, OP_WRITE},
    port,
};

use crate::flash::{Flash, MkSend};

/// Give up on a client that goes quiet for this long. Lost fragments can
/// back TCP's retransmissions off for a long while, so be patient.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
/// A multiple of the flash write size
const CHUNK: usize = 1024;

type FlashPartition = Partition<'static, ThreadModeRawMutex, MkSend<Flash>>;

/// Set if we booted into an update that hasn't been confirmed yet. The state
/// says the same thing between accepting an update and rebooting into it, and
/// a confirm then must not cancel the update.
static UNCONFIRMED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct DfuConfig {
    internal: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>,
    state_start: u32,
    state_end: u32,
    dfu_start: u32,
    dfu_end: u32,
    active_len: u32,
}

impl DfuConfig {
    pub fn new(internal: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>) -> Self {
        extern "C" {
            static __bootloader_state_start: u32;
            static __bootloader_state_end: u32;
            static __bootloader_dfu_start: u32;
            static __bootloader_dfu_end: u32;
            static __config_start: u32;
        }

        unsafe {
            let state_end = &__bootloader_state_end as *const u32 as u32;

            Self {
                internal,
                state_start: &__bootloader_state_start as *const u32 as u32,
                state_end,
                dfu_start: &__bootloader_dfu_start as *const u32 as u32,
                dfu_end: &__bootloader_dfu_end as *const u32 as u32,
                // the application lives between the bootloader state and the config
                active_len: &__config_start as *const u32 as u32 - state_end,
            }
        }
    }

    pub fn state(&self) -> FlashPartition {
        Partition::new(
            self.internal,
            self.state_start,
            self.state_end - self.state_start,
        )
    }

    pub fn dfu(&self) -> FlashPartition {
        Partition::new(self.internal, self.dfu_start, self.dfu_end - self.dfu_start)
    }
}

impl<N: ErrorType> ErrorType for MkSend<N> {
    type Error = N::Error;
}

impl<N: ReadNorFlash> ReadNorFlash for MkSend<N> {
    const READ_SIZE: usize = N::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read(offset, bytes).await
    }

    fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl<N: NorFlash> NorFlash for MkSend<N> {
    const WRITE_SIZE: usize = N::WRITE_SIZE;

    const ERASE_SIZE: usize = N::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.0.erase(from, to).await
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.write(offset, bytes).await
    }
}

pub async fn init(spawner: &Spawner, stack: Stack<'static>, config: DfuConfig) {
    let mut magic = AlignedBuffer([0; 4]);
    let mut state = FirmwareState::new(config.state(), &mut magic.0);

    match state.get_state().await {
        Ok(State::Swap) => {
            crate::log::info!("Running a new image, it'll be rolled back unless it's confirmed");
            UNCONFIRMED.store(true, Ordering::Relaxed);
        }
        Ok(State::Revert) => {
            // the update was rolled back, so this is the image we trust, and
            // updates aren't accepted until it's marked as booted
            crate::log::warn!("An update was rolled back");
            if let Err(e) = state.mark_booted().await {
                crate::log::error!("Failed to mark booted: {}", defmt::Debug2Format(&e));
            }
        }
        Ok(_) => {}
        Err(e) => crate::log::error!(
            "Failed to read firmware state: {}",
            defmt::Debug2Format(&e)
        ),
    }

    spawner.spawn(server(stack, config).unwrap());
}

#[embassy_executor::task]
async fn server(stack: Stack<'static>, config: DfuConfig) {
    let mut listener = TcpListener::new(stack).unwrap();
    listener.listen(port::DFU).unwrap();

    let mut rx_buf = [0u8; 2048];
    let mut tx_buf = [0u8; 64];

    loop {
        let token = match listener.accept().await {
            Ok(token) => token,
            Err(e) => {
                crate::log::warn!("DFU accept failed: {}", e);
                continue;
            }
        };

        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        socket.set_timeout(Some(CLIENT_TIMEOUT));
        if socket.accept(token).await.is_err() {
            continue;
        }

        crate::log::info!("DFU client connected: {}", socket.remote_addr());

        let (status, reboot) = match handle(&mut socket, &config).await {
            Ok(op) => op,
            Err(()) => {
                socket.abort();
                continue;
            }
        };

        crate::log::info!("DFU finished with: {}", status);

        _ = socket.write_all(&[status as u8]).await;
        socket.close();
        _ = with_timeout(Duration::from_secs(1), socket.flush()).await;

        if reboot {
            crate::log::info!("Going down for update");
            Timer::after_millis(300).await;
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}

/// Returns the status to send back, and whether to reboot after sending it.
/// Errors if the client went away.
async fn handle(socket: &mut TcpSocket<'_, '_>, config: &DfuConfig) -> Result<(Status, bool), ()> {
    let mut header = [0u8; 5];
    socket.read_exact(&mut header).await.map_err(|_| ())?;

    if header[..4] != MAGIC {
        return Ok((Status::BadRequest, false));
    }

    match header[4] {
        OP_CONFIRM => Ok((confirm(config).await, false)),
        OP_WRITE => {
            let mut lens = [0u8; 8];
            socket.read_exact(&mut lens).await.map_err(|_| ())?;
            let len = u32::from_le_bytes(lens[..4].try_into().unwrap());
            let crc = u32::from_le_bytes(lens[4..].try_into().unwrap());

            let status = write(socket, config, len, crc).await?;
            Ok((status, status == Status::Ok))
        }
        _ => Ok((Status::BadRequest, false)),
    }
}

async fn confirm(config: &DfuConfig) -> Status {
    if !UNCONFIRMED.load(Ordering::Relaxed) {
        return Status::Ok;
    }

    let mut magic = AlignedBuffer([0; 4]);
    let mut state = FirmwareState::new(config.state(), &mut magic.0);

    match state.get_state().await {
        Ok(State::Swap) => match state.mark_booted().await {
            Ok(()) => {
                crate::log::info!("New image confirmed");
                UNCONFIRMED.store(false, Ordering::Relaxed);
                Status::Ok
            }
            Err(_) => Status::FlashError,
        },
        Ok(_) => Status::Ok,
        Err(_) => Status::FlashError,
    }
}

/// Read and drop `len` bytes
async fn discard(socket: &mut TcpSocket<'_, '_>, len: u32) -> Result<(), ()> {
    let mut buf = [0u8; 256];
    let mut left = len as usize;
    while left > 0 {
        let n = buf.len().min(left);
        socket.read_exact(&mut buf[..n]).await.map_err(|_| ())?;
        left -= n;
    }
    Ok(())
}

async fn write(
    socket: &mut TcpSocket<'_, '_>,
    config: &DfuConfig,
    len: u32,
    expected_crc: u32,
) -> Result<Status, ()> {
    // the client sends the whole image before it reads our answer, so take
    // it all even if we're turning it down
    let refusal = if UNCONFIRMED.load(Ordering::Relaxed) {
        Some(Status::Unconfirmed)
    } else if len == 0 || len > config.active_len {
        Some(Status::TooLarge)
    } else {
        None
    };

    if let Some(status) = refusal {
        discard(socket, len).await?;
        return Ok(status);
    }

    crate::log::info!("Receiving a {} byte image", len);

    let mut aligned = AlignedBuffer([0; 4]);
    let mut updater = FirmwareUpdater::new(
        FirmwareUpdaterConfig {
            dfu: config.dfu(),
            state: config.state(),
        },
        &mut aligned.0,
    );

    let mut chunk = [0u8; CHUNK];
    let mut offset = 0usize;
    let mut failed = false;

    while offset < len as usize {
        let want = CHUNK.min(len as usize - offset);
        socket
            .read_exact(&mut chunk[..want])
            .await
            .map_err(|_| ())?;

        // keep reading even if writing failed, so the client gets an answer
        if failed {
            offset += want;
            continue;
        }

        // the tail needs padding out to the flash write size
        let padded = want.next_multiple_of(4);
        chunk[want..padded].fill(0xff);

        if let Err(e) = updater.write_firmware(offset, &chunk[..padded]).await {
            crate::log::error!("Failed to write firmware: {}", defmt::Debug2Format(&e));
            failed = true;
        }

        offset += want;
    }

    if failed {
        return Ok(Status::FlashError);
    }

    // check what actually made it to flash
    let mut crc = crc32fast::Hasher::new();
    let mut offset = 0usize;
    while offset < len as usize {
        let n = CHUNK.min(len as usize - offset);
        if updater.read_dfu(offset as u32, &mut chunk[..n]).await.is_err() {
            return Ok(Status::FlashError);
        }
        crc.update(&chunk[..n]);
        offset += n;
    }

    if crc.finalize() != expected_crc {
        return Ok(Status::CrcMismatch);
    }

    if updater.mark_updated().await.is_err() {
        return Ok(Status::FlashError);
    }

    Ok(Status::Ok)
}
