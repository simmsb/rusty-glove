// https://github.com/embassy-rs/nrf-softdevice/blob/991d28e3c61687908acc53e748191eb59d00e27a/examples/src/bin/ble_keyboard_peripheral_builder_macro.rs
//
// Copyright (c) 2020 Dario Nieuwenhuis

// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:

// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![allow(unused)]

use nrf_softdevice::{
    ble::{
        gatt_server::{
            builder::ServiceBuilder,
            characteristic::{Attribute, Metadata, Properties},
            CharacteristicHandles, RegisterError,
        },
        Uuid,
    },
    Softdevice,
};

const DEVICE_INFORMATION: Uuid = Uuid::new_16(0x180a);
const BATTERY_SERVICE: Uuid = Uuid::new_16(0x180f);

const BATTERY_LEVEL: Uuid = Uuid::new_16(0x2a19);
const MODEL_NUMBER: Uuid = Uuid::new_16(0x2a24);
const SERIAL_NUMBER: Uuid = Uuid::new_16(0x2a25);
const FIRMWARE_REVISION: Uuid = Uuid::new_16(0x2a26);
const HARDWARE_REVISION: Uuid = Uuid::new_16(0x2a27);
const SOFTWARE_REVISION: Uuid = Uuid::new_16(0x2a28);
const MANUFACTURER_NAME: Uuid = Uuid::new_16(0x2a29);
const PNP_ID: Uuid = Uuid::new_16(0x2a50);

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum VidSource {
    BluetoothSIG = 1,
    UsbIF = 2,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PnPID {
    pub vid_source: VidSource,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_version: u16,
}

#[derive(Debug, Default, defmt::Format)]
pub struct DeviceInformation {
    pub manufacturer_name: Option<&'static str>,
    pub model_number: Option<&'static str>,
    pub serial_number: Option<&'static str>,
    pub hw_rev: Option<&'static str>,
    pub fw_rev: Option<&'static str>,
    pub sw_rev: Option<&'static str>,
}

pub struct DeviceInformationService {}

impl DeviceInformationService {
    pub fn new(
        sd: &mut Softdevice,
        pnp_id: Option<&PnPID>,
        info: DeviceInformation,
    ) -> Result<Self, RegisterError> {
        let mut sb = ServiceBuilder::new(sd, DEVICE_INFORMATION)?;

        if let Some(pnp_id) = pnp_id {
            Self::add_pnp_characteristic(&mut sb, pnp_id)?;
        }
        Self::add_opt_str_characteristic(&mut sb, MANUFACTURER_NAME, info.manufacturer_name)?;
        Self::add_opt_str_characteristic(&mut sb, MODEL_NUMBER, info.model_number)?;
        Self::add_opt_str_characteristic(&mut sb, SERIAL_NUMBER, info.serial_number)?;
        Self::add_opt_str_characteristic(&mut sb, HARDWARE_REVISION, info.hw_rev)?;
        Self::add_opt_str_characteristic(&mut sb, FIRMWARE_REVISION, info.fw_rev)?;
        Self::add_opt_str_characteristic(&mut sb, SOFTWARE_REVISION, info.sw_rev)?;

        let _service_handle = sb.build();

        Ok(DeviceInformationService {})
    }

    fn add_opt_str_characteristic(
        sb: &mut ServiceBuilder,
        uuid: Uuid,
        val: Option<&'static str>,
    ) -> Result<Option<CharacteristicHandles>, RegisterError> {
        if let Some(val) = val {
            let attr = Attribute::new(val);
            let md = Metadata::new(Properties::new().read());
            Ok(Some(sb.add_characteristic(uuid, attr, md)?.build()))
        } else {
            Ok(None)
        }
    }

    fn add_pnp_characteristic(
        sb: &mut ServiceBuilder,
        pnp_id: &PnPID,
    ) -> Result<CharacteristicHandles, RegisterError> {
        // SAFETY: `PnPID` is `repr(C, packed)` so viewing it as an immutable slice of bytes is safe.
        let val = unsafe {
            core::slice::from_raw_parts(
                pnp_id as *const _ as *const u8,
                core::mem::size_of::<PnPID>(),
            )
        };

        let attr = Attribute::new(val);
        let md = Metadata::new(Properties::new().read());
        Ok(sb.add_characteristic(PNP_ID, attr, md)?.build())
    }
}
