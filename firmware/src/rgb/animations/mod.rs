use cichlid::ColorRGB;
use embassy_time::Duration;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::rng::MyRng;

use super::{animation::Animation, layout::Light};

pub mod null;
pub mod perlin;
pub mod rain;
pub mod snow;

pub enum DynAnimation {
    Snow(snow::Snow),
    Perlin(perlin::Perlin),
    Rain(rain::Rain),
    Null(null::Null),
}

impl DynAnimation {
    pub fn random() -> Self {
        const OPTS: &[fn() -> DynAnimation] = &[
            || DynAnimation::Snow(snow::Snow::default()),
            || DynAnimation::Perlin(perlin::Perlin::default()),
            || DynAnimation::Rain(rain::Rain::default()),
        ];
        OPTS.choose(&mut MyRng).unwrap()()
    }
}

macro_rules! dyn_impl {
    ($([$variant:ident, $anim:ty]),+) => {
        impl Animation for DynAnimation {
            type SyncMessage = AnimationSync;

            fn construct_sync(&self) -> Self::SyncMessage {
                match self {
                    $(
                        Self::$variant(x) => x.wrap_sync()
                    ),+
                }
            }

            fn tick_rate(&self) -> Duration {
                match self {
                    $(
                        Self::$variant(x) => x.tick_rate()
                    ),+
                }
            }

            fn tick(&mut self) {
                match self {
                    $(
                        Self::$variant(x) => x.tick()
                    ),+
                }
            }

            fn render(&self, light: &Light) -> ColorRGB {
                match self {
                    $(
                        Self::$variant(x) => x.render(light)
                    ),+
                }
            }

            fn sync(&mut self, sync: Self::SyncMessage) {
                #[allow(unreachable_patterns)]
                match (self, sync) {
                    $(
                        (Self::$variant(x), AnimationSync::$variant(s)) => x.sync(s)
                    ),+,
                    _ => ()
                }
            }

            fn new_from_sync(sync: Self::SyncMessage) -> Self {
                match sync {
                    $(
                        AnimationSync::$variant(x) => DynAnimation::$variant(<$anim>::new_from_sync(x))
                    ),+
                }
            }
        }

        impl DynAnimation {
            pub fn is(&self, sync: &AnimationSync) -> bool {
                #[allow(unreachable_patterns)]
                match (self, sync) {
                    $(
                        (Self::$variant(_x), AnimationSync::$variant(_s)) => true
                    ),+,
                    _ => false
                }
            }

            pub fn name(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant(_) => stringify!($variant)
                    ),+
                }
            }
        }
    };
}

dyn_impl!(
    [Snow, snow::Snow],
    [Perlin, perlin::Perlin],
    [Rain, rain::Rain],
    [Null, null::Null]
);

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
#[cfg_attr(feature = "probe", derive(defmt::Format))]
pub enum AnimationSync {
    Snow(
        #[cfg_attr(feature = "probe", defmt(Debug2Format))] <snow::Snow as Animation>::SyncMessage,
    ),
    Perlin(
        #[cfg_attr(feature = "probe", defmt(Debug2Format))]
        <perlin::Perlin as Animation>::SyncMessage,
    ),
    Null(
        #[cfg_attr(feature = "probe", defmt(Debug2Format))] <null::Null as Animation>::SyncMessage,
    ),
    Rain(
        #[cfg_attr(feature = "probe", defmt(Debug2Format))] <rain::Rain as Animation>::SyncMessage,
    ),
}

trait WrapAnimationSync {
    fn wrap_sync(&self) -> AnimationSync;
}

macro_rules! wrap_sync {
    ($anim:ty, $variant:expr) => {
        impl WrapAnimationSync for $anim {
            fn wrap_sync(&self) -> AnimationSync {
                $variant(self.construct_sync())
            }
        }
    };
}

wrap_sync!(snow::Snow, AnimationSync::Snow);
wrap_sync!(perlin::Perlin, AnimationSync::Perlin);
wrap_sync!(null::Null, AnimationSync::Null);
wrap_sync!(rain::Rain, AnimationSync::Rain);

#[derive(
    serde::Serialize,
    serde::Deserialize,
    Clone,
    Copy,
    Debug,
    Hash,
    Eq,
    PartialEq,
    postcard::experimental::max_size::MaxSize,
)]
pub struct ColorRGBWire {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<cichlid::ColorRGB> for ColorRGBWire {
    fn from(cichlid::ColorRGB { r, g, b }: cichlid::ColorRGB) -> Self {
        Self { r, g, b }
    }
}

impl From<ColorRGBWire> for cichlid::ColorRGB {
    fn from(ColorRGBWire { r, g, b }: ColorRGBWire) -> Self {
        cichlid::ColorRGB { r, g, b }
    }
}
