use core::hash::Hash;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Copy, Debug)]
#[repr(u8)]
#[derive(postcard::experimental::max_size::MaxSize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum KeyboardSide {
    Left,
    Right,
}

impl KeyboardSide {
    pub const fn is_left(self) -> bool {
        matches!(self, Self::Left)
    }

    pub const fn is_right(self) -> bool {
        matches!(self, Self::Right)
    }

    pub const fn other(self) -> Self {
        match self {
            KeyboardSide::Left => KeyboardSide::Right,
            KeyboardSide::Right => KeyboardSide::Left,
        }
    }
}
