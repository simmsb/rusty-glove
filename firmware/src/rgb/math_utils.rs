#![allow(dead_code)]

use cichlid::ColorRGB;
use fixed_macro::fixed;

use fixed::types::{I16F16, U0F16, U16F16};
use palette::{blend::Blend, convert::FromColorUnclamped, LinSrgb, Mix, Okhsv, OklabHue};
use rand::Rng;

use crate::rng::MyRng;

pub(crate) fn blend(a: ColorRGB, b: ColorRGB, between: u8) -> ColorRGB {
    let a = palette::Oklab::from_color_unclamped(LinSrgb::<f32>::from(LinSrgb::new(a.r, a.g, a.b)));
    let b = palette::Oklab::from_color_unclamped(LinSrgb::<f32>::from(LinSrgb::new(b.r, b.g, b.b)));

    let c = a.mix(b, between as f32 / 256.0);
    let c: LinSrgb<u8> = LinSrgb::from_color_unclamped(c).into();

    ColorRGB {
        r: c.red,
        g: c.green,
        b: c.blue,
    }
}

pub(crate) fn overlay(a: ColorRGB, b: ColorRGB) -> ColorRGB {
    let a = LinSrgb::<f32>::from(LinSrgb::new(a.r, a.g, a.b));
    let b = LinSrgb::<f32>::from(LinSrgb::new(b.r, b.g, b.b));

    let c = a.screen(b);
    let c: LinSrgb<u8> = LinSrgb::from_color_unclamped(c).into();

    ColorRGB {
        r: c.red,
        g: c.green,
        b: c.blue,
    }
}

pub(crate) fn wrapping_delta(a: I16F16, b: I16F16, min: I16F16, max: I16F16) -> I16F16 {
    let half_range = (max - min) / fixed!(2: I16F16);

    let d = b.wrapping_sub(a);

    if d.abs() <= half_range {
        d
    } else {
        b.wrapping_sub(max).wrapping_add(min.wrapping_sub(a))
    }
}

pub(crate) fn wrapping_delta_u(a: U16F16, b: U16F16, min: U16F16, max: U16F16) -> U16F16 {
    let half_range = (max - min) / fixed!(2: U16F16);

    let d = b.abs_diff(a);

    if d <= half_range {
        d
    } else {
        half_range - d
    }
}

pub(crate) fn sqr(x: I16F16) -> I16F16 {
    x * x
}

pub(crate) fn rand_decimal() -> f32 {
    MyRng.gen()
}

pub(crate) fn rand_rainbow() -> ColorRGB {
    rainbow(rand_decimal())
}

pub(crate) fn rainbow(x: f32) -> ColorRGB {
    let c = Okhsv::new(
        OklabHue::from_degrees((x - 0.5) * 360.0),
        1.0,
        1.0,
    );

    let c: LinSrgb<u8> = LinSrgb::from_color_unclamped(c).into();

    ColorRGB {
        r: c.red,
        g: c.green,
        b: c.blue,
    }
}

pub(crate) fn ease_fade(pct: U0F16) -> u8 {
    let mix = if pct < fixed!(0.5: U0F16) {
        let pct: I16F16 = pct.to_num();
        2 * pct * pct
    } else {
        let pct: I16F16 = pct.to_num();
        let a = fixed!(-2: I16F16) * pct + fixed!(2: I16F16);
        let b = a * a;
        fixed!(1: I16F16) - b / 2
    };

    mix.lerp(fixed!(0: I16F16), fixed!(255: I16F16))
        .int()
        .saturating_to_num()
}
