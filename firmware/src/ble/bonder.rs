use core::cell::RefCell;

use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use trouble_host::prelude::*;

use super::{server::GloveServer, BleStack, Pool};
use crate::sync::WaitCell;

const BONDS_MAX: usize = 8;

/// The identity address of a peer, as returned by [`Address::to_bytes`]
type BondKey = [u8; 7];

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct StoredBond {
    info: BondInformation,
    /// The CCCD state of the peer, so it doesn't need to resubscribe after we
    /// reboot
    att_table: heapless::Vec<u8, { trouble_host::config::CLIENT_ATT_TABLE_SIZE }>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Clone, Debug)]
struct SavedBonds(heapless::Vec<BondKey, BONDS_MAX>);

#[derive(serde::Serialize)]
struct StoredBondKey(BondKey);

static BONDS: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::LinearMap<BondKey, StoredBond, BONDS_MAX>>,
> = Mutex::new(RefCell::new(heapless::LinearMap::new()));
static BONDS_UPDATE: WaitCell = WaitCell::new();

fn key_of(identity: &Identity) -> BondKey {
    identity.addr.to_bytes()
}

/// Load the saved bonds from flash and hand them to the stack
pub async fn load_bonds(stack: &BleStack) {
    let keys = crate::flash::get::<SavedBonds>().await;

    crate::log::debug!(
        "Loaded bond information with {} entries",
        keys.as_ref().map(|x| x.0.len()).unwrap_or(0)
    );

    for key in keys.iter().flat_map(|s| s.0.iter()) {
        let Some(bond) = crate::flash::get_keyed::<_, StoredBond>(StoredBondKey(*key)).await else {
            crate::log::warn!(
                "Bonder tried to load info for addr: {} but it was missing?",
                key
            );
            continue;
        };

        if let Err(e) = stack.add_bond_information(bond.info.clone()) {
            crate::log::warn!("Failed to restore bond: {}", defmt::Debug2Format(&e));
            continue;
        }

        BONDS.lock(|bonds| {
            _ = bonds.borrow_mut().insert(*key, bond);
        });
    }
}

#[embassy_executor::task]
pub async fn save_bondinfo_loop() {
    loop {
        _ = BONDS_UPDATE.wait().await;

        let data = BONDS.lock(|x| x.borrow().clone());

        let prior_keys = crate::flash::get::<SavedBonds>().await;

        for key in prior_keys.iter().flat_map(|s| s.0.iter()) {
            if !data.contains_key(key) {
                crate::flash::delete_keyed::<StoredBondKey, StoredBond>(StoredBondKey(*key)).await;
            }
        }

        let keys = SavedBonds(data.keys().copied().collect());

        crate::log::debug!("Saving bond information with {} keys", keys.0.len());

        crate::flash::set(&keys).await;

        for (k, v) in data.iter() {
            crate::flash::set_keyed(StoredBondKey(*k), v).await;
        }
    }
}

/// Record a new (or refreshed) bond
pub fn on_bonded(
    stack: &BleStack,
    server: &GloveServer<'_>,
    conn: &Connection<'_, Pool>,
    bond: BondInformation,
) {
    let key = key_of(&bond.identity);
    crate::log::trace!("Bonded with {}", key);

    let att_table = att_table_of(server, conn);

    let evicted = BONDS.lock(|bonds| {
        let mut bonds = bonds.borrow_mut();
        let mut evicted = None;

        if !bonds.contains_key(&key) && bonds.len() == bonds.capacity() {
            // forget the oldest bond to make room
            if let Some((&oldest, _)) = bonds.iter().next() {
                evicted = bonds.remove(&oldest);
            }
        }

        // don't lose saved subscriptions if the server doesn't have any for
        // this peer yet
        let att_table = match bonds.get(&key) {
            Some(existing) if att_table.is_empty() => existing.att_table.clone(),
            _ => att_table,
        };

        _ = bonds.insert(
            key,
            StoredBond {
                info: bond,
                att_table,
            },
        );

        evicted
    });

    if let Some(evicted) = evicted {
        _ = stack.remove_bond_information(evicted.info.identity);
    }

    BONDS_UPDATE.wake();
}

fn att_table_of(
    server: &GloveServer<'_>,
    conn: &Connection<'_, Pool>,
) -> heapless::Vec<u8, { trouble_host::config::CLIENT_ATT_TABLE_SIZE }> {
    server
        .get_client_att_table(conn)
        .and_then(|t| heapless::Vec::from_slice(t.raw()).ok())
        .unwrap_or_default()
}

/// Remember the CCCD state of a bonded peer
pub fn save_att_table(server: &GloveServer<'_>, conn: &Connection<'_, Pool>) {
    let key = key_of(&conn.peer_identity());
    let att_table = att_table_of(server, conn);

    let changed = BONDS.lock(|bonds| {
        let mut bonds = bonds.borrow_mut();
        let Some(bond) = bonds.get_mut(&key) else {
            return false;
        };

        if bond.att_table == att_table {
            return false;
        }

        bond.att_table = att_table;
        true
    });

    if changed {
        BONDS_UPDATE.wake();
    }
}

/// Restore the CCCD state of a bonded peer
pub fn load_att_table(server: &GloveServer<'_>, conn: &Connection<'_, Pool>) {
    let key = key_of(&conn.peer_identity());

    let Some(att_table) = BONDS.lock(|bonds| Some(bonds.borrow().get(&key)?.att_table.clone()))
    else {
        return;
    };

    crate::log::debug!("Restoring att table for {}", key);

    match ClientAttTableView::try_from_raw(&att_table) {
        Ok(view) => server.set_client_att_table(conn, &view),
        Err(e) => crate::log::trace!("Error restoring att table: {}", defmt::Debug2Format(&e)),
    }
}
