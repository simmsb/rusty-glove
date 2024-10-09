use crate::{flash::MkSend, log::*};
use embassy_embedded_hal::flash::partition::Partition;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embedded_storage::nor_flash::ErrorType;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use heapless::Vec;
use nrf_dfu_target::prelude::*;
use nrf_softdevice::{
    ble::{gatt_server::NotifyValueError, Connection},
    Flash,
};

pub const MTU: usize = 120;
// Aligned to 4 bytes + 3 bytes for header
pub const ATT_MTU: usize = MTU + 3;

pub type Target = DfuTarget<256>;

#[nrf_softdevice::gatt_service(uuid = "FE59")]
pub struct NrfDfuService {
    #[characteristic(
        uuid = "8EC90001-F315-4F60-9FB8-838830DAEA50",
        write,
        notify,
        security = "justworks"
    )]
    control: Vec<u8, ATT_MTU>,

    /// The maximum size of each packet is derived from the Att MTU size of the connection.
    /// The maximum Att MTU size of the DFU Service is 256 bytes (saved in NRF_SDH_BLE_GATT_MAX_MTU_SIZE),
    /// making the maximum size of the DFU Packet characteristic 253 bytes. (3 bytes are used for opcode and handle ID upon writing.)
    #[characteristic(
        uuid = "8EC90002-F315-4F60-9FB8-838830DAEA50",
        write_without_response,
        notify,
        security = "justworks"
    )]
    packet: Vec<u8, ATT_MTU>,
}

pub struct ConnectionHandle {
    pub connection: Connection,
    pub notify_control: bool,
    pub notify_packet: bool,
}

impl NrfDfuService {
    async fn process<
        DFU: NorFlash,
        F: FnOnce(&ConnectionHandle, &[u8]) -> Result<(), NotifyValueError>,
    >(
        &self,
        target: &mut Target,
        dfu: &mut DFU,
        conn: &mut ConnectionHandle,
        request: DfuRequest<'_>,
        notify: F,
    ) -> DfuStatus {
        let (response, status) = target.process(request, dfu).await;
        let mut buf: [u8; 32] = [0; 32];
        match response.encode(&mut buf[..]) {
            Ok(len) => match notify(&conn, &buf[..len]) {
                Ok(_) => {}
                Err(e) => {
                    warn!("Error sending notification: {:?}", e);
                }
            },
            Err(e) => {
                warn!("Error encoding DFU response: {:?}", e);
            }
        }
        status
    }

    pub async fn handle<DFU: NorFlash>(
        &self,
        target: &mut Target,
        dfu: &mut DFU,
        connection: &mut ConnectionHandle,
        event: NrfDfuServiceEvent,
    ) -> Option<DfuStatus> {
        match event {
            NrfDfuServiceEvent::ControlWrite(data) => {
                if let Ok((request, _)) = DfuRequest::decode(&data) {
                    return Some(
                        self.process(target, dfu, connection, request, |conn, response| {
                            if conn.notify_control {
                                self.control_notify(
                                    &conn.connection,
                                    &Vec::from_slice(response).unwrap(),
                                )?;
                            }
                            Ok(())
                        })
                        .await,
                    );
                }
            }
            NrfDfuServiceEvent::ControlCccdWrite { notifications } => {
                connection.notify_control = notifications;
            }
            NrfDfuServiceEvent::PacketWrite(data) => {
                let request = DfuRequest::Write { data: &data[..] };
                return Some(
                    self.process(target, dfu, connection, request, |conn, response| {
                        if conn.notify_control {
                            self.control_notify(
                                &conn.connection,
                                &Vec::from_slice(response).unwrap(),
                            )?;
                        }
                        // if conn.notify_packet {
                        //     self.packet_notify(&conn.connection, &Vec::from_slice(response).unwrap())?;
                        // }
                        Ok(())
                    })
                    .await,
                );
            }
            NrfDfuServiceEvent::PacketCccdWrite { notifications } => {
                connection.notify_packet = notifications;
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct DfuConfig {
    internal: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>,
    state_start: u32,
    state_end: u32,
    dfu_start: u32,
    dfu_end: u32,
}

impl DfuConfig {
    pub fn new(internal: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>) -> Self {
        extern "C" {
            static __bootloader_state_start: u32;
            static __bootloader_state_end: u32;
            static __bootloader_dfu_start: u32;
            static __bootloader_dfu_end: u32;
        }

        unsafe {
            let dfu_start = &__bootloader_dfu_start as *const u32 as u32;
            let dfu_end = &__bootloader_dfu_end as *const u32 as u32;

            Partition::new(internal, dfu_start, dfu_end - dfu_start);

            let state_start = &__bootloader_state_start as *const u32 as u32;
            let state_end = &__bootloader_state_end as *const u32 as u32;

            Partition::new(internal, state_start, state_end - state_start);
            Self {
                internal,
                state_start,
                state_end,
                dfu_start,
                dfu_end,
            }
        }
    }

    pub fn state(&self) -> Partition<'static, ThreadModeRawMutex, MkSend<Flash>> {
        Partition::new(
            self.internal,
            self.state_start,
            self.state_end - self.state_start,
        )
    }

    pub fn dfu(&self) -> Partition<'static, ThreadModeRawMutex, MkSend<Flash>> {
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
