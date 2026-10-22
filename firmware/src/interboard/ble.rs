use trouble_host::prelude::Address;

// these must match the uuids of `SplitService`
pub const INTERBOARD_SERVER_UUID: u128 = 0xcb6dcd5e_7f1b_11ef_8c55_a71ac707ac76;
pub const INTERBOARD_SERVER_TO_CENTRAL: u128 = 0x2e72e4a2_7f1c_11ef_a1c2_13ed5ddd22d6;
pub const INTERBOARD_SERVER_TO_PERIPHERAL: u128 = 0x3969f044_7f1c_11ef_96c9_7ba31210e355;

/// The fixed address of the non-master side, so the master can find it
pub fn peripheral_address() -> Address {
    Address::random([0x18, 0xe2, 0x21, 0x80, 0xc0, 0xc7])
}
