use core::convert::Infallible;

use embedded_hal_0_2::digital::v2::{InputPin, OutputPin};

const THUMB_CLUSTER_PATCH_R: [(u8, u8); 6] = [(8, 6), (7, 6), (6, 6), (8, 7), (7, 7), (6, 7)];
const THUMB_CLUSTER_PATCH_L: [(u8, u8); 6] = [(3, 6), (4, 6), (5, 6), (3, 7), (4, 7), (5, 7)];

// patch the thumb cluster keys to end up on two rows beneath the main keys
//
// doing this because I'm too lazy to update my keymap generator
fn patch_pos(x: u8, y: u8) -> (u8, u8) {
    if crate::side::get_side().is_right() {
        if x == 0 {
            THUMB_CLUSTER_PATCH_R[y as usize]
        } else {
            (x + 5, y)
        }
    } else {
        if x == 6 {
            THUMB_CLUSTER_PATCH_L[y as usize]
        } else {
            (x, y)
        }
    }
}

pub struct Scanner<C, R>
where
    <C as ScanMatrix<R>>::Debouncers: Default,
    C: ScanMatrix<R>,
    R: ScanRows,
{
    cols: C,
    rows: R,
    debouncers: <C as ScanMatrix<R>>::Debouncers,
}

impl<C, R> Scanner<C, R>
where
    <C as ScanMatrix<R>>::Debouncers: Default,
    C: ScanMatrix<R>,
    R: ScanRows,
{
    pub fn new(cols: C, rows: R) -> Self {
        Self {
            cols,
            rows,
            debouncers: Default::default(),
        }
    }

    pub fn scan(&mut self) -> impl Iterator<Item = keyberon::layout::Event> {
        let scan_result = self.cols.scan_matrix(&self.rows, &mut self.debouncers);

        scan_result.into_iter().enumerate().flat_map(|(j, row)| {
            row.into_iter()
                .enumerate()
                .filter_map(move |(i, press_state)| {
                    press_state.map(|press| {
                        let (x, y) = patch_pos(j as u8, i as u8);
                        if press {
                            crate::log::debug!("kp: ({}, {}) (orig: ({}, {}))", x, y, j, i);
                            keyberon::layout::Event::Press(y, x)
                        } else {
                            keyberon::layout::Event::Release(y, x)
                        }
                    })
                })
        })
    }
}

pub trait ScanRows {
    type Result: IntoIterator<Item = Option<bool>>;
    type Debouncers;

    fn scan_rows(&self, debouncers: &mut Self::Debouncers) -> Self::Result;
}

const DEBOUNCE_PERIOD: u8 = 40; // polling at 1000hz

impl<C0, C1, C2, C3, C4, C5> ScanRows for (C0, C1, C2, C3, C4, C5)
where
    C0: InputPin<Error = Infallible>,
    C1: InputPin<Error = Infallible>,
    C2: InputPin<Error = Infallible>,
    C3: InputPin<Error = Infallible>,
    C4: InputPin<Error = Infallible>,
    C5: InputPin<Error = Infallible>,
{
    type Result = [Option<bool>; 6];
    type Debouncers = [Debouncer<DEBOUNCE_PERIOD>; 6];

    fn scan_rows(&self, debouncers: &mut Self::Debouncers) -> Self::Result {
        cortex_m::asm::delay(1000);
        [
            debouncers[0].update(self.0.is_high().unwrap()),
            debouncers[1].update(self.1.is_high().unwrap()),
            debouncers[2].update(self.2.is_high().unwrap()),
            debouncers[3].update(self.3.is_high().unwrap()),
            debouncers[4].update(self.4.is_high().unwrap()),
            debouncers[5].update(self.5.is_high().unwrap()),
        ]
    }
}

pub trait ScanMatrix<C: ScanRows> {
    type Result: IntoIterator<Item = C::Result>;
    type Debouncers;

    fn scan_matrix(&mut self, columns: &C, debouncers: &mut Self::Debouncers) -> Self::Result;
}

impl<R, C0, C1, C2, C3, C4, C5, C6> ScanMatrix<R> for (C0, C1, C2, C3, C4, C5, C6)
where
    R: ScanRows,
    C0: OutputPin<Error = Infallible>,
    C1: OutputPin<Error = Infallible>,
    C2: OutputPin<Error = Infallible>,
    C3: OutputPin<Error = Infallible>,
    C4: OutputPin<Error = Infallible>,
    C5: OutputPin<Error = Infallible>,
    C6: OutputPin<Error = Infallible>,
{
    type Result = [R::Result; 7];
    type Debouncers = [R::Debouncers; 7];

    fn scan_matrix(&mut self, rows: &R, debouncers: &mut Self::Debouncers) -> Self::Result {
        self.0.set_high().unwrap();
        let a = rows.scan_rows(&mut debouncers[0]);
        self.0.set_low().unwrap();

        self.1.set_high().unwrap();
        let b = rows.scan_rows(&mut debouncers[1]);
        self.1.set_low().unwrap();

        self.2.set_high().unwrap();
        let c = rows.scan_rows(&mut debouncers[2]);
        self.2.set_low().unwrap();

        self.3.set_high().unwrap();
        let d = rows.scan_rows(&mut debouncers[3]);
        self.3.set_low().unwrap();

        self.4.set_high().unwrap();
        let e = rows.scan_rows(&mut debouncers[4]);
        self.4.set_low().unwrap();

        self.5.set_high().unwrap();
        let f = rows.scan_rows(&mut debouncers[5]);
        self.5.set_low().unwrap();

        self.6.set_high().unwrap();
        let g = rows.scan_rows(&mut debouncers[6]);
        self.6.set_low().unwrap();

        [a, b, c, d, e, f, g]
    }
}

pub struct Debouncer<const MAX: u8> {
    timer: u8,
    is_pressed: bool,
}

impl<const MAX: u8> Default for Debouncer<MAX> {
    fn default() -> Self {
        Self {
            timer: 0,
            is_pressed: false,
        }
    }
}

impl<const MAX: u8> Debouncer<MAX> {
    fn update(&mut self, is_pressed: bool) -> Option<bool> {
        self.timer = self.timer.saturating_sub(1);

        if is_pressed {
            self.pressed()
        } else {
            self.unpressed()
        }
    }

    fn unpressed(&mut self) -> Option<bool> {
        if self.timer == 0 && self.is_pressed {
            self.timer = MAX;
            self.is_pressed = false;
            return Some(false);
        }

        None
    }

    fn pressed(&mut self) -> Option<bool> {
        if self.timer == 0 && !self.is_pressed {
            self.timer = MAX;
            self.is_pressed = true;

            return Some(true);
        }

        None
    }
}
