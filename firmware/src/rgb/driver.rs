use bytemuck::{Pod, Zeroable};
use cichlid::ColorRGB;
use embassy_nrf::{
    gpio::{OutputDrive, Pin as GpioPin},
    pwm::{
        Config, Instance, Prescaler, SequenceConfig, SequenceLoad, SequencePwm, SingleSequenceMode,
        SingleSequencer,
    },
    Peripheral,
};
use embassy_time::Timer;

// const NUM_INITIAL_RESETS: usize = 10;
// const NUM_RESETS: usize = 10;
// const NUM_SAMPLES: usize = NUM_LEDS as usize * 3 + NUM_RESETS + NUM_INITIAL_RESETS;

// static RGB_TO_I2S: [i32; 256] = {
//     let mut table = [0i32; 256];

//     let mut i = 0;
//     while i < 256 {
//         let mut val = 0x88888888u32;

//         let mut n = 0u8;
//         while n < 8 {
//             if (i as u8 >> n) & 1 == 1 {
//                 let mask = !(0xf << (4 * n as u32));
//                 let patt = 0xe << (4 * n as u32);

//                 val = (val & mask) | patt;
//             }

//             n += 1;
//         }

//         table[i] = ((val >> 16) | (val << 16)) as i32;

//         i += 1;
//     }

//     table
// };

// static RGB_TO_I2S: [[i16; 2]; 256] = {
//     let mut table = [[0i16; 2]; 256];

//     let mut i = 0;
//     while i < 256 {
//         let mut val = 0x88888888u32;

//         let mut n = 0u8;
//         while n < 8 {
//             if (i as u8 >> n) & 1 == 1 {
//                 let mask = !(0xf << (4 * n as u32));
//                 let patt = 0xe << (4 * n as u32);

//                 val = (val & mask) | patt;
//             }

//             n += 1;
//         }

//         table[i][0] = (val >> 16) as i16;
//         table[i][1] = (val & 0xffff) as i16;

//         i += 1;
//     }

//     table
// };

// pub struct Ws2812<P: i2s::Instance> {
//     i2s: OutputStream<'static, P, i32, 1, NUM_SAMPLES>
// }

// impl<P: i2s::Instance> Ws2812<P> {
//     pub fn new(i2s: P,
//                irq: impl Binding<P::Interrupt, i2s::InterruptHandler<P>> + 'static,
//                sdout: impl GpioPin,
//                mck_unused: impl GpioPin,
//                sck_unused: impl GpioPin,
//                lrck_unused: impl GpioPin,
//     ) -> Self {
//         let master_clock = MasterClock::new(i2s::MckFreq::_32MDiv10, i2s::Ratio::_32x);
//         let config = i2s::Config::default();
//         let i2s = i2s::I2S::new_master(i2s, irq, mck_unused, sck_unused, lrck_unused, master_clock, config);
//         let output = i2s.output(sdout, MultiBuffering::new());
//         Self { i2s: output }
//     }

//     pub async fn write(&mut self, colors: &[ColorRGB; NUM_LEDS as usize]) {
//         self.i2s.start().await.unwrap();

//         let buf = self.i2s.buffer();

//         for (dst, color) in buf[NUM_INITIAL_RESETS..].array_chunks_mut::<3>().zip(colors) {
//             // dst[0] = RGB_TO_I2S[color.g as usize];
//             // dst[1] = RGB_TO_I2S[color.r as usize];
//             dst[2] = RGB_TO_I2S[color.b as usize];
//         }

//         for dst in &mut buf[..NUM_INITIAL_RESETS] {
//             *dst = 0;
//         }

//         for dst in &mut buf[(NUM_INITIAL_RESETS + NUM_LEDS as usize * 3)..] {
//             *dst = 0;
//         }

//         self.i2s.send().await.unwrap();
//         self.i2s.stop().await;
//     }
// }

pub struct Ws2812<P: Instance, const N: usize> {
    pwm: SequencePwm<'static, P>,
}

const T1H: u16 = 0x8000 | 13; // Duty = 13/20 ticks (0.8us/1.25us) for a 1
const T0H: u16 = 0x8000 | 7; // Duty 7/20 ticks (0.4us/1.25us) for a 0
const RES: u16 = 0x8000;

static RGB_TO_PWM: [[u16; 8]; 256] = {
    let mut table = [[0u16; 8]; 256];

    let mut i = 0;
    while i < 256 {
        let mut n = 0u8;
        while n < 8 {
            table[i as usize][7 - n as usize] = if (i as u8 >> n) & 1 == 1 { T1H } else { T0H };
            n += 1;
        }

        i += 1;
    }

    table
};

#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
#[repr(C)]
struct RGB {
    g: [u16; 8],
    r: [u16; 8],
    b: [u16; 8],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Seq<const N: usize> {
    rgbs: [RGB; N],
    fin: u16,
}

unsafe impl<const N: usize> Pod for Seq<N> {}
unsafe impl<const N: usize> Zeroable for Seq<N> {}

impl<P: Instance, const N: usize> Ws2812<P, N> {
    pub fn new<Pin: Peripheral<P = impl GpioPin> + 'static>(pwm: P, pin: Pin) -> Self {
        let mut config = Config::default();
        config.ch0_drive = OutputDrive::HighDrive;
        config.sequence_load = SequenceLoad::Common;
        config.prescaler = Prescaler::Div1;
        config.max_duty = 20; // 1.25us (1s / 16Mhz * 20)
        let pwm = SequencePwm::new_1ch(pwm, pin, config).unwrap();
        Self { pwm }
    }

    pub async fn write(&mut self, colors: &[ColorRGB; N]) {
        let mut buf = Seq::<N>::zeroed();
        buf.fin = RES;

        for (dst, color) in buf.rgbs.iter_mut().zip(colors) {
            dst.r = RGB_TO_PWM[color.r as usize];
            dst.g = RGB_TO_PWM[color.g as usize];
            dst.b = RGB_TO_PWM[color.b as usize];
        }

        // let mut x = 0;
        // for dst in buf.rgbs.iter_mut() {
        //     dst.r = RGB_TO_PWM[255 - (x * 6)];
        //     dst.g = RGB_TO_PWM[x * 6];
        //     dst.b = RGB_TO_PWM[0];
        //     x += 1;
        // }

        let seq: &[u16] = bytemuck::cast_slice(bytemuck::bytes_of(&buf));

        let mut seq_config = SequenceConfig::default();
        seq_config.end_delay = 799; // 50us (20 ticks * 40) - 1 tick because we've already got one RES;

        let sequences = SingleSequencer::new(&mut self.pwm, seq, seq_config);
        sequences.start(SingleSequenceMode::Times(1)).unwrap();

        // Timer::after_micros(const { (1.25 * (3.0 * 8.0 * N as f32 + 1.0 + 100.0)) as u64 }).await;
        Timer::after_millis(8).await;
    }
}
