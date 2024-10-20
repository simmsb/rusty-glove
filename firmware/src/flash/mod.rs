use core::any::TypeId;

use ekv::flash::{self, PageID};
use ekv::{config, Database};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use nrf_softdevice::{Flash, FlashError};
use once_cell::sync::OnceCell;
use rand::Rng;

use crate::rng::MyRng;

pub struct MkSend<T>(pub T);

unsafe impl<T> Send for MkSend<T> {}

static DB: OnceCell<Database<DbFlash, ThreadModeRawMutex>> = OnceCell::new();

pub async fn init(flash: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>) {
    let flash = DbFlash {
        flash,
        start: unsafe { &__config_start as *const u32 as usize },
    };
    let mut cfg = ekv::Config::default();
    cfg.random_seed = MyRng.gen();
    let db = Database::new(flash, cfg);

    if db.mount().await.is_err() && db.format().await.is_err() {
        return;
    }

    DB.set(db).ok().unwrap();
}

async fn get_db() -> &'static Database<DbFlash, ThreadModeRawMutex> {
    loop {
        if let Some(db) = DB.get() {
            return db;
        }

        // we could do some fancy waitqueue system here or actually
        // topologically order task startups but this is simpler
        Timer::after_millis(10).await;
    }
}

pub async fn set<T: core::any::Any + serde::Serialize>(value: &T) -> Option<()> {
    let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];
    let buf = postcard::to_slice(value, &mut buf).ok()?;
    let mut tx = get_db().await.write_transaction().await;

    // convert the typeid of the key to a byte array
    let key = unsafe {
        core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(TypeId::of::<T>())
    };

    tx.write(&key, buf).await.ok()?;
    tx.commit().await.ok()?;

    Some(())
}

pub async fn get<T: core::any::Any + serde::de::DeserializeOwned>() -> Option<T> {
    let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];

    let tx = get_db().await.read_transaction().await;

    // convert the typeid of the key to a byte array
    let key = unsafe {
        core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(TypeId::of::<T>())
    };

    let len = tx.read(&key, &mut buf).await.ok()?;

    postcard::from_bytes(&buf[..len]).ok()
}

extern "C" {
    // u32 as align is 4
    static __config_start: u32;
    static __config_end: u32;
}

// Workaround for alignment requirements.
#[repr(C, align(4))]
struct AlignedBuf<const N: usize>([u8; N]);

struct DbFlash {
    start: usize,
    flash: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>,
}

impl flash::Flash for DbFlash {
    type Error = FlashError;

    fn page_count(&self) -> usize {
        (unsafe { (&__config_end as *const _ as usize) - (&__config_start as *const _ as usize) })
            / config::PAGE_SIZE
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), <DbFlash as flash::Flash>::Error> {
        let mut guard = self.flash.lock().await;
        guard
            .0
            .erase(
                (self.start + page_id.index() * config::PAGE_SIZE) as u32,
                (self.start + page_id.index() * config::PAGE_SIZE + config::PAGE_SIZE) as u32,
            )
            .await
    }

    async fn read(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), <DbFlash as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        let mut guard = self.flash.lock().await;
        guard
            .0
            .read(address as u32, &mut buf.0[..data.len()])
            .await?;
        data.copy_from_slice(&buf.0[..data.len()]);
        Ok(())
    }

    async fn write(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), <DbFlash as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        buf.0[..data.len()].copy_from_slice(data);
        let mut guard = self.flash.lock().await;
        guard.0.write(address as u32, &buf.0[..data.len()]).await
    }
}
