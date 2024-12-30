pub const NUM_LEDS: u16 = 40;
pub const NUM_COLS: usize = 9;
pub const MAX_LED_XPOS: usize = 7;
pub const MAX_LED_YPOS: usize = 6;

#[derive(Clone, Copy)]
pub struct Light {
    /// relative distance from the bottom left light on the left board (mm)
    pub location: (i16, i16),

    /// (raw) matrix position
    pub position: (u8, u8),

    pub index: u16,
}

struct UnindexedLight {
    pub location: (i16, i16),
    pub position: (u8, u8),
}

impl UnindexedLight {
    const fn switch(location: (i16, i16), position: (u8, u8)) -> Self {
        assert!((position.0 as usize) < MAX_LED_XPOS);
        assert!((position.1 as usize) < MAX_LED_YPOS);

        Self { location, position }
    }
}

pub struct Lights {
    pub lights: [Light; NUM_LEDS as usize],
    pub inverse_index: [[u16; MAX_LED_YPOS]; MAX_LED_XPOS],
}

const fn index_lights(lights: [UnindexedLight; NUM_LEDS as usize]) -> Lights {
    let mut out: [MaybeUninit<Light>; NUM_LEDS as usize] = MaybeUninit::uninit_array();
    let mut inverse_index: [[u16; MAX_LED_YPOS]; MAX_LED_XPOS] = [[0; MAX_LED_YPOS]; MAX_LED_XPOS];

    let mut i = 0;
    while i < NUM_LEDS as usize {
        let light = &lights[i];

        out[i].write(Light {
            location: light.location,
            position: light.position,
            index: i as u16,
        });

        let (x, y) = light.position;

        inverse_index[x as usize][y as usize] = i as u16;

        i += 1;
    }

    // gets spooked otherwise
    core::mem::forget(lights);
    let lights = unsafe { MaybeUninit::array_assume_init(out) };

    Lights {
        lights,
        inverse_index,
    }
}

pub mod left {
    use super::{index_lights, Lights, UnindexedLight, NUM_COLS};

    /// the top right switch in the left keyboard is offset in the x axis by this much
    const TOP_RIGHT_LED_OFFSET: i16 = 90;

    const fn s(x: i16, y: i16, mx: u8, my: u8) -> UnindexedLight {
        UnindexedLight::switch((TOP_RIGHT_LED_OFFSET - x, y), (mx, my))
    }

    // we use the same relative positions as the right side, just flipped and
    // shifted

    pub const LEFT: Lights = index_lights([
        // thumb cluster
        s(40, 5, 6, 0),
        s(20, 0, 6, 1),
        s(0, -5, 6, 2),
        s(40, -5, 6, 3),
        s(20, -15, 6, 4),
        s(0, -25, 6, 5),
        // col 0
        s(60, 80, 5, 1),
        s(60, 60, 5, 2),
        s(60, 40, 5, 3),
        s(60, 20, 5, 4),
        // col 1
        s(80, 100, 4, 0),
        s(80, 80, 4, 1),
        s(80, 60, 4, 2),
        s(80, 40, 4, 3),
        s(80, 20, 4, 4),
        s(80, 0, 4, 5),
        // col 2
        s(100, 100, 3, 0),
        s(100, 80, 3, 1),
        s(100, 60, 3, 2),
        s(100, 40, 3, 3),
        s(100, 20, 3, 4),
        s(100, 0, 3, 5),
        // col 3
        s(120, 100, 2, 0),
        s(120, 80, 2, 1),
        s(120, 60, 2, 2),
        s(120, 40, 2, 3),
        s(120, 20, 2, 4),
        s(120, 0, 2, 5),
        // col 4
        s(140, 100, 1, 0),
        s(140, 80, 1, 1),
        s(140, 60, 1, 2),
        s(140, 40, 1, 3),
        s(140, 20, 1, 4),
        s(140, 0, 1, 5),
        // col 5
        s(160, 100, 0, 0),
        s(160, 80, 0, 1),
        s(160, 60, 0, 2),
        s(160, 40, 0, 3),
        s(160, 20, 0, 4),
        s(160, 0, 0, 5),
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
    use super::{index_lights, Lights, UnindexedLight, NUM_COLS};

    /// the top left switch in the right keyboard is offset in the x axis by this much
    const RIGHT_LED_OFFSET: i16 = 180;

    const fn s(x: i16, y: i16, mx: u8, my: u8) -> UnindexedLight {
        UnindexedLight::switch((x + RIGHT_LED_OFFSET, y), (mx, my))
    }

    pub const RIGHT: Lights = index_lights([
        // thumb cluster
        s(40, 5, 0, 0),
        s(20, 0, 0, 1),
        s(0, -5, 0, 2),
        s(40, -5, 0, 3),
        s(20, -15, 0, 4),
        s(0, -25, 0, 5),
        // col 0
        s(60, 80, 1, 1),
        s(60, 60, 1, 2),
        s(60, 40, 1, 3),
        s(60, 20, 1, 4),
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
