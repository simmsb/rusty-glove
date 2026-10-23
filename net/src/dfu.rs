//! Firmware updates over the network, for any node running the embassy
//! bootloader. The protocol is described in [`shared::net::dfu`].
//!
//! The node's `memory.x` must lay flash out the way the bootloader's does, and
//! define the `__bootloader_state_*`, `__bootloader_dfu_*` and `__config_start`
//! symbols (the application runs from the end of the state partition up to the
//! config partition).

use embassy_boot::{AlignedBuffer, FirmwareState, FirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_embedded_hal::flash::partition::Partition;
use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    wire::ListenSocketAddr,
    Stack,
};
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_time::{with_timeout, Duration, Timer};
use embedded_io_async::{Read, Write};
use embedded_storage_async::nor_flash::NorFlash;
use portable_atomic::{AtomicBool, Ordering};
use shared::net::dfu::{Status, MAGIC, OP_CONFIRM, OP_WRITE};

macro_rules! info {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::info!($($t)*);
    }};
}

macro_rules! warn {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::warn!($($t)*);
    }};
}

macro_rules! error {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::error!($($t)*);
    }};
}

/// Give up on a client that goes quiet for this long. Lost fragments can
/// back TCP's retransmissions off for a long while, so be patient.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
/// A multiple of the flash write size
const CHUNK: usize = 1024;

/// Set if we booted into an update that hasn't been confirmed yet. The state
/// says the same thing between accepting an update and rebooting into it, and
/// a confirm then must not cancel the update.
static UNCONFIRMED: AtomicBool = AtomicBool::new(false);

pub struct DfuConfig<M: RawMutex + 'static, F: 'static> {
    flash: &'static Mutex<M, F>,
    state_start: u32,
    state_end: u32,
    dfu_start: u32,
    dfu_end: u32,
    active_len: u32,
}

impl<M: RawMutex, F> Clone for DfuConfig<M, F> {
    fn clone(&self) -> Self {
        Self { ..*self }
    }
}

impl<M: RawMutex, F: NorFlash> DfuConfig<M, F> {
    /// Find the partitions from the linker script, see the module docs
    pub fn from_linkerfile(flash: &'static Mutex<M, F>) -> Self {
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
                flash,
                state_start: &__bootloader_state_start as *const u32 as u32,
                state_end,
                dfu_start: &__bootloader_dfu_start as *const u32 as u32,
                dfu_end: &__bootloader_dfu_end as *const u32 as u32,
                active_len: &__config_start as *const u32 as u32 - state_end,
            }
        }
    }

    fn state(&self) -> Partition<'static, M, F> {
        Partition::new(self.flash, self.state_start, self.state_end - self.state_start)
    }

    fn dfu(&self) -> Partition<'static, M, F> {
        Partition::new(self.flash, self.dfu_start, self.dfu_end - self.dfu_start)
    }
}

/// See how we booted, call before [`serve`]
pub async fn check_boot<M: RawMutex, F: NorFlash>(config: &DfuConfig<M, F>) {
    let mut magic = AlignedBuffer([0; 4]);
    let mut state = FirmwareState::new(config.state(), &mut magic.0);

    match state.get_state().await {
        Ok(State::Swap) => {
            info!("Running a new image, it'll be rolled back unless it's confirmed");
            UNCONFIRMED.store(true, Ordering::Relaxed);
        }
        Ok(State::Revert) => {
            // the update was rolled back, so this is the image we trust, and
            // updates aren't accepted until it's marked as booted
            warn!("An update was rolled back");
            if let Err(_e) = state.mark_booted().await {
                error!("Failed to mark booted: {}", defmt::Debug2Format(&_e));
            }
        }
        Ok(_) => {}
        Err(_e) => error!("Failed to read firmware state: {}", defmt::Debug2Format(&_e)),
    }
}

/// Take updates on `local`, rebooting into each one that's written
pub async fn serve<M: RawMutex, F: NorFlash>(
    stack: Stack<'_>,
    local: impl Into<ListenSocketAddr>,
    config: DfuConfig<M, F>,
) -> ! {
    let mut listener = TcpListener::new(stack).unwrap();
    listener.listen(local).unwrap();

    let mut rx_buf = [0u8; 2048];
    let mut tx_buf = [0u8; 64];

    loop {
        let token = match listener.accept().await {
            Ok(token) => token,
            Err(_e) => {
                warn!("DFU accept failed: {}", _e);
                continue;
            }
        };

        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        socket.set_timeout(Some(CLIENT_TIMEOUT));
        if socket.accept(token).await.is_err() {
            continue;
        }

        info!("DFU client connected: {}", socket.remote_addr());

        let (status, reboot) = match handle(&mut socket, &config).await {
            Ok(op) => op,
            Err(()) => {
                socket.abort();
                continue;
            }
        };

        info!("DFU finished with: {}", status);

        _ = socket.write_all(&[status as u8]).await;
        socket.close();
        _ = with_timeout(Duration::from_secs(1), socket.flush()).await;

        if reboot {
            info!("Going down for update");
            Timer::after_millis(300).await;
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}

/// Returns the status to send back, and whether to reboot after sending it.
/// Errors if the client went away.
async fn handle<M: RawMutex, F: NorFlash>(
    socket: &mut TcpSocket<'_, '_>,
    config: &DfuConfig<M, F>,
) -> Result<(Status, bool), ()> {
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

async fn confirm<M: RawMutex, F: NorFlash>(config: &DfuConfig<M, F>) -> Status {
    if !UNCONFIRMED.load(Ordering::Relaxed) {
        return Status::Ok;
    }

    let mut magic = AlignedBuffer([0; 4]);
    let mut state = FirmwareState::new(config.state(), &mut magic.0);

    match state.get_state().await {
        Ok(State::Swap) => match state.mark_booted().await {
            Ok(()) => {
                info!("New image confirmed");
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

async fn write<M: RawMutex, F: NorFlash>(
    socket: &mut TcpSocket<'_, '_>,
    config: &DfuConfig<M, F>,
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

    info!("Receiving a {} byte image", len);

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

        if let Err(_e) = updater.write_firmware(offset, &chunk[..padded]).await {
            error!("Failed to write firmware: {}", defmt::Debug2Format(&_e));
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
