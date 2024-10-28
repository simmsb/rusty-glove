use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use nrf_softdevice::ble::{self, gatt_server::get_sys_attrs, security::SecurityHandler};
use once_cell::sync::OnceCell;

use crate::sync::WaitCell;

macro_rules! mkfrom {
    (@conv, $x:expr, $f:ident, i) => {
        $x.$f.into()
    };
    (@conv, $x:expr, $f:ident, ) => {
        $x.$f
    };
    ($a:path => $b:path: [$( $f:ident $(: $i:ident)? ),* $(,)?]) => {
        impl From<$a> for $b {
            fn from(a: $a) -> Self {
                Self {
                    $( $f: mkfrom!(@conv, a, $f, $($i)?) , )*
                }
            }
        }

        impl Into<$a> for $b {
            fn into(self) -> $a {
                $a {
                    $( $f: mkfrom!(@conv, self, $f, $($i)?), )*
                }
            }
        }
    };
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct EncryptionInfo {
    ltk: [u8; 16],
    flags: u8,
}

mkfrom!(ble::EncryptionInfo => EncryptionInfo: [ ltk, flags ]);

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct MasterId {
    ediv: u16,
    rand: [u8; 8],
}

mkfrom!(ble::MasterId => MasterId: [ ediv, rand ]);

const SYS_ATTRS_MAX_LEN: usize = 64;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct SysAttrs {
    data: heapless::Vec<u8, SYS_ATTRS_MAX_LEN>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Copy, Clone, Debug, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct Address {
    flags: u8,
    bytes: [u8; 6],
}

mkfrom!(ble::Address => Address: [ flags, bytes ]);

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct IdentityResolutionKey {
    irk: [u8; 16],
}

impl From<ble::IdentityResolutionKey> for IdentityResolutionKey {
    fn from(value: ble::IdentityResolutionKey) -> Self {
        Self {
            irk: value.as_raw().irk,
        }
    }
}

impl From<IdentityResolutionKey> for ble::IdentityResolutionKey {
    fn from(val: IdentityResolutionKey) -> Self {
        ble::IdentityResolutionKey::from_raw(nrf_softdevice_s140::ble_gap_irk_t { irk: val.irk })
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct IdentityKey {
    irk: IdentityResolutionKey,
    addr: Address,
}

mkfrom!(ble::IdentityKey => IdentityKey: [ irk:i, addr:i ]);

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct BondInfo {
    encryption: EncryptionInfo,
    master_id: MasterId,
    identity_key: IdentityKey,
    sys_attrs: SysAttrs,
}

pub struct Bonder {
    data: embassy_sync::blocking_mutex::Mutex<
        CriticalSectionRawMutex,
        RefCell<heapless::LinearMap<Address, BondInfo, 8>>,
    >,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Clone, Debug)]
struct SavedBonds(heapless::FnvIndexSet<Address, 8>);

#[derive(serde::Serialize)]
struct BondKey(Address);

pub async fn load_bonder() -> &'static Bonder {
    if let Some(x) = BONDER.get() {
        return x;
    }

    let keys = crate::flash::get::<SavedBonds>().await;

    crate::log::debug!("Loaded bond information with {} entries", keys.as_ref().map(|x| x.0.len()).unwrap_or(0));

    let mut map = heapless::LinearMap::new();

    for addr in keys.iter().flat_map(|s| s.0.iter()) {
        if let Some(info) = crate::flash::get_keyed(BondKey(*addr)).await {
            crate::log::debug!("{} = {:#?}", addr, info);
            _ = map.insert(*addr, info);
        } else {
            crate::log::warn!("Bonder tried to load info for addr: {} but it was missing?", addr);
        }
    }

    let bonder = Bonder {
        data: embassy_sync::blocking_mutex::Mutex::new(RefCell::new(map)),
    };

    BONDER.get_or_init(move || bonder)
}

pub static BONDER: OnceCell<Bonder> = OnceCell::new();
static BONDER_UPDATE: WaitCell = WaitCell::new();

#[embassy_executor::task]
pub async fn save_bondinfo_loop(bonder: &'static Bonder) {
    loop {
        _ = BONDER_UPDATE.wait().await;

        let data = bonder.data.lock(|x| x.borrow().clone());

        let prior_keys = crate::flash::get::<SavedBonds>().await;

        for addr in prior_keys.iter().flat_map(|s| s.0.iter()) {
            crate::flash::delete_keyed::<BondKey, BondInfo>(BondKey(*addr)).await;
        }

        let keys = SavedBonds(data.keys().copied().collect());

        crate::log::debug!("Saving bond information with {} keys", keys.0.len());

        crate::flash::set(&keys).await;

        for (k, v) in data.iter() {
            crate::log::debug!("{} = {:#?}", k, v);

            crate::flash::set_keyed(BondKey(*k), v).await;
        }
    }
}

// this is a horrible single peer implementation, I'll tidy it up later

impl SecurityHandler for Bonder {
    fn io_capabilities(&self) -> nrf_softdevice::ble::security::IoCapabilities {
        nrf_softdevice::ble::security::IoCapabilities::None
    }

    fn can_bond(&self, _conn: &nrf_softdevice::ble::Connection) -> bool {
        true
    }

    fn on_security_update(
        &self,
        _conn: &nrf_softdevice::ble::Connection,
        _security_mode: nrf_softdevice::ble::SecurityMode,
    ) {
        crate::log::debug!("on_security_update {}", _security_mode);
    }

    fn on_bonded(
        &self,
        conn: &nrf_softdevice::ble::Connection,
        master_id: nrf_softdevice::ble::MasterId,
        key: nrf_softdevice::ble::EncryptionInfo,
        peer_id: nrf_softdevice::ble::IdentityKey,
    ) {
        crate::log::trace!("Bonded with {}", Address::from(conn.peer_address()));
        self.data.lock(|bonds| {
            let mut bonds = bonds.borrow_mut();
            if !bonds.contains_key(&peer_id.addr.into()) {
                if bonds.len() == bonds.capacity() {
                    let Some((&to_clear, _)) = bonds.iter().next() else {
                        return;
                    };

                    bonds.remove(&to_clear);
                }

                _ = bonds.insert(
                    peer_id.addr.into(),
                    BondInfo {
                        encryption: key.into(),
                        master_id: master_id.into(),
                        identity_key: peer_id.into(),
                        sys_attrs: SysAttrs::default(),
                    },
                );
            } else if let Some(x) = bonds.get_mut(&peer_id.addr.into()) {
                x.encryption = key.into();
                x.master_id = master_id.into();
                x.identity_key = peer_id.into();
            }
        });
        self.save_sys_attrs(conn);
    }

    fn get_key(
        &self,
        conn: &nrf_softdevice::ble::Connection,
        master_id: nrf_softdevice::ble::MasterId,
    ) -> Option<nrf_softdevice::ble::EncryptionInfo> {
        crate::log::trace!("Getting key for {}", Address::from(conn.peer_address()));

        let Some((saved_master_id, encryption_info)) = self.data.lock(|bonds| {
            let bonds = bonds.borrow();
            let info = bonds.get(&conn.peer_address().into())?;

            Some((info.master_id.clone(), info.encryption.clone()))
        }) else {
            let data = self.data.lock(|bonds| bonds.borrow().clone());

            crate::log::trace!("Nothing found for key, we have {} entries:", data.len());

            for (key, val) in data.iter() {
                crate::log::trace!("{} = {:#?}", key, val);
            }

            return None;
        };

        crate::log::trace!(
            "Got master id: {} and encryption: {}",
            saved_master_id,
            encryption_info
        );

        if Into::<ble::MasterId>::into(saved_master_id) != master_id {
            crate::log::debug!(
                "Found no bond info for {} (saved: {})",
                master_id,
                saved_master_id
            );

            return None;
        }

        Some(encryption_info.into())
    }

    fn save_sys_attrs(&self, conn: &nrf_softdevice::ble::Connection) {
        let mut buf = [0u8; SYS_ATTRS_MAX_LEN];
        let Ok(len) = get_sys_attrs(conn, &mut buf) else {
            crate::log::error!("Get sys attrs failed");
            return;
        };

        let attrs = SysAttrs {
            data: heapless::Vec::from_slice(&buf[..len]).unwrap(),
        };

        self.data.lock(|bonds| {
            let mut bonds = bonds.borrow_mut();
            if let Some(x) = bonds.get_mut(&conn.peer_address().into()) {
                x.sys_attrs = attrs;
            }
        });

        BONDER_UPDATE.wake();
    }

    fn load_sys_attrs(&self, conn: &nrf_softdevice::ble::Connection) {
        let attrs = self.data.lock(|bonds| {
            let bonds = bonds.borrow();
            let info = bonds.get(&conn.peer_address().into())?;

            Some(info.sys_attrs.clone())
        });

        crate::log::debug!("Getting sys attrs for {}: {}", conn.peer_address(), attrs);

        if let Err(err) = nrf_softdevice::ble::gatt_server::set_sys_attrs(
            conn,
            attrs.as_ref().map(|a| a.data.as_slice()),
        ) {
            crate::log::trace!("Error setting sys attrs: {}", err);
        }
    }
}
