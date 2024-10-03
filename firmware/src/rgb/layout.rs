pub const NUM_LEDS: u16 = 40;
pub const NUM_COLS: usize = 9;

#[derive(Clone, Copy)]
pub struct Light {
    /// relative distance from the bottom left light on the left board (mm)
    pub location: (i16, i16),

    /// matrix position
    pub position: Option<(u8, u8)>,

    pub index: u16,
}

struct UnindexedLight {
    pub location: (i16, i16),
    pub position: Option<(u8, u8)>,
}

impl UnindexedLight {
    const fn switch(location: (i16, i16), position: (u8, u8)) -> Self {
        Self {
            location,
            position: Some(position),
        }
    }
}

const fn index_lights<const N: usize>(lights: [UnindexedLight; N]) -> [Light; N] {
    let mut out: [MaybeUninit<Light>; N] = MaybeUninit::uninit_array();

    let mut i = 0;
    while i < N {
        out[i].write(Light {
            location: lights[i].location,
            position: lights[i].position,
            index: i as u16,
        });

        i += 1;
    }

    // gets spooked otherwise
    core::mem::forget(lights);
    unsafe { MaybeUninit::array_assume_init(out) }
}

pub mod left {
    use super::{index_lights, Light, UnindexedLight, NUM_COLS, NUM_LEDS};

    /// the top right switch in the left keyboard is offset in the x axis by this much
    const TOP_RIGHT_LED_OFFSET: i16 = 90;

    const fn s(x: i16, y: i16, mx: u8, my: u8) -> UnindexedLight {
        UnindexedLight::switch((TOP_RIGHT_LED_OFFSET - x, y), (6 - mx, my))
    }

    // we use the same relative positions as the right side, just flipped and
    // shifted

    pub const LEFT: [Light; NUM_LEDS as usize] = index_lights([
        // thumb cluster
        s(40, 5, 0, 0),
        s(20, 0, 0, 2),
        s(0, -5, 0, 3),
        s(40, -5, 0, 4),
        s(20, -15, 0, 5),
        s(0, -25, 0, 6),
        // col 0
        s(60, 80, 1, 0),
        s(60, 60, 1, 1),
        s(60, 40, 1, 2),
        s(60, 20, 1, 3),
        // col 1
        s(80, 100, 2, 0),
        s(80, 80, 2, 1),
        s(80, 60, 2, 2),
        s(80, 40, 2, 3),
        s(80, 20, 2, 4),
        s(80, 0, 2, 5),
        // col 2
        s(100, 100, 3, 0),
        s(100, 80, 3, 1),
        s(100, 60, 3, 2),
        s(100, 40, 3, 3),
        s(100, 20, 3, 4),
        s(100, 0, 4, 5),
        // col 3
        s(120, 100, 4, 0),
        s(120, 80, 4, 1),
        s(120, 60, 4, 2),
        s(120, 40, 4, 3),
        s(120, 20, 4, 4),
        s(120, 0, 4, 5),
        // col 4
        s(140, 100, 5, 0),
        s(140, 80, 5, 1),
        s(140, 60, 5, 2),
        s(140, 40, 5, 3),
        s(140, 20, 5, 4),
        s(140, 0, 5, 5),
        // col 5
        s(160, 100, 6, 0),
        s(160, 80, 6, 1),
        s(160, 60, 6, 2),
        s(160, 40, 6, 3),
        s(160, 20, 6, 4),
        s(160, 0, 6, 5),
    ]);

    const fn c(x: i16) -> i16 {
        TOP_RIGHT_LED_OFFSET - x
    }

    pub const COLUMNS: [i16; NUM_COLS] = [
        c(0),
        c(20),
        c(40),
        c(60),
        c(80),
        c(100),
        c(120),
        c(140),
        c(160),
    ];
}

use core::mem::MaybeUninit;

pub use left::LEFT;

pub mod right {
    use super::{index_lights, Light, UnindexedLight, NUM_COLS, NUM_LEDS};

    /// the top left switch in the right keyboard is offset in the x axis by this much
    const RIGHT_LED_OFFSET: i16 = 180;
    const RIGHT_MATRIX_OFFSET: u8 = 5;

    const fn s(x: i16, y: i16, mx: u8, my: u8) -> UnindexedLight {
        UnindexedLight::switch((x + RIGHT_LED_OFFSET, y), (mx + RIGHT_MATRIX_OFFSET, my))
    }

    pub const RIGHT: [Light; NUM_LEDS as usize] = index_lights([
        // thumb cluster
        s(40, 5, 0, 0),
        s(20, 0, 0, 2),
        s(0, -5, 0, 3),
        s(40, -5, 0, 4),
        s(20, -15, 0, 5),
        s(0, -25, 0, 6),
        // col 0
        s(60, 80, 1, 0),
        s(60, 60, 1, 1),
        s(60, 40, 1, 2),
        s(60, 20, 1, 3),
        // col 1
        s(80, 100, 2, 0),
        s(80, 80, 2, 1),
        s(80, 60, 2, 2),
        s(80, 40, 2, 3),
        s(80, 20, 2, 4),
        s(80, 0, 2, 5),
        // col 2
        s(100, 100, 3, 0),
        s(100, 80, 3, 1),
        s(100, 60, 3, 2),
        s(100, 40, 3, 3),
        s(100, 20, 3, 4),
        s(100, 0, 4, 5),
        // col 3
        s(120, 100, 4, 0),
        s(120, 80, 4, 1),
        s(120, 60, 4, 2),
        s(120, 40, 4, 3),
        s(120, 20, 4, 4),
        s(120, 0, 4, 5),
        // col 4
        s(140, 100, 5, 0),
        s(140, 80, 5, 1),
        s(140, 60, 5, 2),
        s(140, 40, 5, 3),
        s(140, 20, 5, 4),
        s(140, 0, 5, 5),
        // col 5
        s(160, 100, 6, 0),
        s(160, 80, 6, 1),
        s(160, 60, 6, 2),
        s(160, 40, 6, 3),
        s(160, 20, 6, 4),
        s(160, 0, 6, 5),
    ]);

    const fn c(x: i16) -> i16 {
        x + RIGHT_LED_OFFSET
    }

    pub const COLUMNS: [i16; NUM_COLS] = [
        c(0),
        c(20),
        c(40),
        c(60),
        c(80),
        c(100),
        c(120),
        c(140),
        c(160),
    ];
}

pub use right::RIGHT;
