use core::any::TypeId;

use ekv::flash::{self, PageID};
use ekv::{config, Database};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use heapless::Vec;
use nrf_softdevice::{Flash, FlashError};
use once_cell::sync::OnceCell;
use rand::Rng;

use crate::rng::MyRng;

pub struct MkSend<T>(pub T);

unsafe impl<T> Send for MkSend<T> {}

static DB: OnceCell<Database<DbFlash, ThreadModeRawMutex>> = OnceCell::new();

async fn init_inner(
    flash: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>,
    do_fmt: bool,
) -> Database<DbFlash, ThreadModeRawMutex> {
    let flash = DbFlash {
        flash,
        start: unsafe { &__config_start as *const u32 as usize },
    };

    crate::log::debug!(
        "Starting up database with {} pages of {}",
        flash::Flash::page_count(&flash),
        config::PAGE_SIZE
    );

    let mut cfg = ekv::Config::default();
    cfg.random_seed = MyRng.gen();
    let db = Database::new(flash, cfg);

    if let Err(err) = db.mount().await {
        crate::log::warn!(
            "Failed to mount (now going to format) with reason: {}",
            defmt::Debug2Format(&err)
        );

        if let Err(err) = db.format().await {
            defmt::panic!(
                "Failed to format with reason: {}",
                defmt::Debug2Format(&err)
            );
        }
    }

    if do_fmt {
        _ = db.format().await;
    }

    db
}

enum TypeIdProbe {}

pub async fn init(flash: &'static Mutex<ThreadModeRawMutex, MkSend<Flash>>) {
    crate::log::info!("Initialising flash and database");
    let mut db = init_inner(flash, false).await;

    if async {
        const PROBE_KEY: &[u8] = b"type-id-probe";
        let tx = db.read_transaction().await;

        let expected = unsafe {
            core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(
                TypeId::of::<TypeIdProbe>(),
            )
        };

        let mut v = [0u8; core::mem::size_of::<TypeId>()];
        match tx.read(PROBE_KEY, &mut v).await {
            Ok(len) => {
                if expected != v[..len] {
                    crate::log::warn!(
                        "Type id probe failed, expecting: {}, got: {}",
                        expected,
                        v[..len]
                    );

                    return None;
                }
            }
            Err(ekv::ReadError::KeyNotFound) => {}
            Err(_) => return None,
        };

        drop(tx);

        // check we can write a key
        let mut tx = db.write_transaction().await;

        tx.write(PROBE_KEY, &expected).await.ok()?;
        tx.commit().await.ok();

        Some(())
    }
    .await
    .is_none()
    {
        crate::log::warn!("Storage was full, issuing a format!");
        core::mem::drop(db);
        db = init_inner(flash, true).await;
        _ = defmt::dbg!(db.format().await);
    }

    DB.set(db).ok().unwrap();

    crate::log::info!("Flash and database initialised");
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

async fn set_inner<K: serde::Serialize, V: core::any::Any + serde::Serialize>(
    type_key: TypeId,
    value_key: Option<K>,
    value: &V,
) -> Option<()> {
    let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];
    let buf = postcard::to_slice(value, &mut buf).ok()?;
    let mut tx = get_db().await.write_transaction().await;

    let mut key_buf = Vec::<u8, { ekv::config::MAX_KEY_SIZE }>::new();

    // convert the typeid of the key to a byte array
    let type_key =
        unsafe { core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(type_key) };

    _ = key_buf.extend_from_slice(&type_key);
    if let Some(v) = value_key {
        key_buf = postcard::to_extend(&v, key_buf).unwrap();
    }

    crate::log::trace!(
        "Setting ({}, {}) ({} bytes) with key: {}",
        core::any::type_name::<K>(),
        core::any::type_name::<V>(),
        buf.len(),
        key_buf
    );

    defmt::dbg!(tx.write(&key_buf, buf).await).ok()?;
    defmt::dbg!(tx.commit().await).ok()?;

    Some(())
}

#[allow(unused)]
pub async fn set<T: core::any::Any + serde::Serialize>(value: &T) -> Option<()> {
    set_inner::<(), T>(TypeId::of::<T>(), None, value).await
}

#[allow(unused)]
pub async fn set_keyed<
    K: core::any::Any + serde::Serialize,
    T: core::any::Any + serde::Serialize,
>(
    key: K,
    value: &T,
) -> Option<()> {
    set_inner::<K, T>(TypeId::of::<(K, T)>(), Some(key), value).await
}

async fn delete_inner<K: serde::Serialize>(type_key: TypeId, value_key: Option<K>) -> Option<()> {
    let mut tx = get_db().await.write_transaction().await;

    let mut key_buf = Vec::<u8, { ekv::config::MAX_KEY_SIZE }>::new();

    // convert the typeid of the key to a byte array
    let type_key =
        unsafe { core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(type_key) };

    _ = key_buf.extend_from_slice(&type_key);
    if let Some(v) = value_key {
        key_buf = postcard::to_extend(&v, key_buf).unwrap();
    }

    crate::log::trace!(
        "Deleting ({}) with key: {}",
        core::any::type_name::<K>(),
        key_buf
    );

    tx.delete(&key_buf).await.ok()?;
    tx.commit().await.ok()?;

    Some(())
}

#[allow(unused)]
pub async fn delete<T: core::any::Any>() -> Option<()> {
    delete_inner::<()>(TypeId::of::<T>(), None).await
}

#[allow(unused)]
pub async fn delete_keyed<K: core::any::Any + serde::Serialize, T: core::any::Any>(
    key: K,
) -> Option<()> {
    delete_inner::<K>(TypeId::of::<(K, T)>(), Some(key)).await
}

async fn get_inner<K: serde::Serialize, T: core::any::Any + serde::de::DeserializeOwned>(
    type_key: TypeId,
    value_key: Option<K>,
) -> Option<T> {
    let mut buf = [0u8; ekv::config::MAX_VALUE_SIZE];

    let tx = get_db().await.read_transaction().await;

    let mut key_buf = Vec::<u8, { ekv::config::MAX_KEY_SIZE }>::new();

    // convert the typeid of the key to a byte array
    let type_key =
        unsafe { core::mem::transmute::<_, [u8; core::mem::size_of::<TypeId>()]>(type_key) };

    _ = key_buf.extend_from_slice(&type_key);
    if let Some(v) = value_key {
        key_buf = postcard::to_extend(&v, key_buf).unwrap();
    }

    crate::log::trace!(
        "Getting ({}, {}) with key: {}",
        core::any::type_name::<K>(),
        core::any::type_name::<T>(),
        key_buf
    );

    let len = tx.read(&key_buf, &mut buf).await.ok()?;

    postcard::from_bytes(&buf[..len]).ok()
}

#[allow(unused)]
pub async fn get<T: core::any::Any + serde::de::DeserializeOwned>() -> Option<T> {
    get_inner::<(), T>(TypeId::of::<T>(), None).await
}

#[allow(unused)]
pub async fn get_keyed<
    K: core::any::Any + serde::Serialize,
    T: core::any::Any + serde::de::DeserializeOwned,
>(
    key: K,
) -> Option<T> {
    get_inner::<K, T>(TypeId::of::<(K, T)>(), Some(key)).await
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
        // crate::log::trace!("Reading {} bytes from flash (page: {}, offset: {})", data.len(), page_id, offset);
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
