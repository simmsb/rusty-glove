use maitake_sync::blocking;
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

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct MasterId {
    ediv: u16,
    rand: [u8; 8],
}

mkfrom!(ble::MasterId => MasterId: [ ediv, rand ]);

const SYS_ATTRS_MAX_LEN: usize = 64;

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct SysAttrs {
    data: heapless::Vec<u8, SYS_ATTRS_MAX_LEN>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
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
            irk: value.as_raw().irk.clone(),
        }
    }
}

impl Into<ble::IdentityResolutionKey> for IdentityResolutionKey {
    fn into(self) -> ble::IdentityResolutionKey {
        ble::IdentityResolutionKey::from_raw(nrf_softdevice_s140::ble_gap_irk_t { irk: self.irk })
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct IdentityKey {
    irk: IdentityResolutionKey,
    addr: Address,
}

mkfrom!(ble::IdentityKey => IdentityKey: [ irk:i, addr:i ]);

pub struct Bonder {
    data: blocking::Mutex<BondInfo>,
}

#[derive(Clone)]
struct BondInfo {
    encryption: Option<EncryptionInfo>,
    master_id: Option<MasterId>,
    identity_key: Option<IdentityKey>,
    sys_attrs: Option<SysAttrs>,
}

pub async fn load_bonder() -> &'static Bonder {
    let bondinfo = BondInfo {
        encryption: crate::flash::get::<EncryptionInfo>().await,
        master_id: crate::flash::get::<MasterId>().await,
        identity_key: crate::flash::get::<IdentityKey>().await,
        sys_attrs: crate::flash::get::<SysAttrs>().await,
    };

    let bonder = Bonder {
        data: blocking::Mutex::new(bondinfo),
    };

    BONDER.get_or_init(move || bonder)
}

static BONDER: OnceCell<Bonder> = OnceCell::new();
static BONDER_UPDATE: WaitCell = WaitCell::new();

#[embassy_executor::task]
pub async fn save_bondinfo_loop(bonder: &'static Bonder) {
    loop {
        _ = BONDER_UPDATE.wait().await;

        let data = bonder.data.with_lock(|x| x.clone());

        if let Some(encryption) = data.encryption.as_ref() {
            crate::flash::set(encryption).await;
        }
        if let Some(master_id) = data.master_id.as_ref() {
            crate::flash::set(master_id).await;
        }
        if let Some(identity_key) = data.identity_key.as_ref() {
            crate::flash::set(identity_key).await;
        }
        if let Some(sys_attrs) = data.sys_attrs.as_ref() {
            crate::flash::set(sys_attrs).await;
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
        self.data.with_lock(|x| {
            x.encryption = Some(key.into());
            x.master_id = Some(master_id.into());
            x.identity_key = Some(peer_id.into());
        });
        self.save_sys_attrs(conn);
    }

    fn get_key(
        &self,
        _conn: &nrf_softdevice::ble::Connection,
        master_id: nrf_softdevice::ble::MasterId,
    ) -> Option<nrf_softdevice::ble::EncryptionInfo> {
        let (saved_master_id, encryption_info) = self
            .data
            .with_lock(|x| Some((x.master_id.clone()?, x.encryption.clone()?)))?;

        if Into::<ble::MasterId>::into(saved_master_id) != master_id {
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

        self.data.with_lock(|x| {
            x.sys_attrs = Some(attrs);
        });

        BONDER_UPDATE.wake();
    }

    fn load_sys_attrs(&self, conn: &nrf_softdevice::ble::Connection) {
        crate::log::debug!("Getting sys attrs for {}", conn.peer_address());

        let attrs = self.data.with_lock(|x| x.sys_attrs.clone());

        if let Err(_err) = nrf_softdevice::ble::gatt_server::set_sys_attrs(
            conn,
            attrs.as_ref().map(|a| a.data.as_slice()),
        ) {}
    }
}
