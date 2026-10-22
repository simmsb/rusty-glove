use core::hash::Hash;
use serde::{Deserialize, Serialize};

use crate::side::KeyboardSide;

#[derive(
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    Hash,
    Clone,
    Debug,
    postcard::experimental::max_size::MaxSize,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceToHost {
    pub from_side: KeyboardSide,
    pub msg: DeviceToHostMsg,
}

#[derive(
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    Hash,
    Clone,
    Debug,
    postcard::experimental::max_size::MaxSize,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DeviceToHostMsg {}
