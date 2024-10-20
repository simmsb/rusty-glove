use nrf_softdevice::ble::Address;

pub const INTERBOARD_SERVER_UUID: &str = "cb6dcd5e-7f1b-11ef-8c55-a71ac707ac76";
pub const INTERBOARD_SERVER_TO_CENTRAL: &str = "2e72e4a2-7f1c-11ef-a1c2-13ed5ddd22d6";
pub const INTERBOARD_SERVER_TO_PERIPHERAL: &str = "3969f044-7f1c-11ef-96c9-7ba31210e355";

pub const CENTRAL_ADDRESS: Address = Address::new(
    nrf_softdevice::ble::AddressType::RandomStatic,
    [0x7e, 0xfe, 0x71, 0x91, 0x11, 0xe3],
);

pub const PERIPHERAL_ADDRESS: Address = Address::new(
    nrf_softdevice::ble::AddressType::RandomStatic,
    [0x18, 0xe2, 0x21, 0x80, 0xc0, 0xc7],
);
