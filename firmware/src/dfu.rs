//! Firmware updates over the network, see [`glove_net::dfu`]

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embedded_storage::nor_flash::ErrorType;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use shared::net::port;

use crate::flash::{Flash, MkSend};

pub type DfuConfig = glove_net::dfu::DfuConfig<ThreadModeRawMutex, MkSend<Flash>>;

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
    glove_net::dfu::check_boot(&config).await;
    spawner.spawn(server(stack, config).unwrap());
}

#[embassy_executor::task]
async fn server(stack: Stack<'static>, config: DfuConfig) {
    glove_net::dfu::serve(stack, port::DFU, config).await
}
