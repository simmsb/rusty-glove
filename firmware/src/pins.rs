macro_rules! define_pin {
    ($name:ident, $p:ident) => {
        #[allow(unused)]
        macro_rules! $name {
            () => {
                ::embassy_nrf::peripherals::$p
            };
        }

        ::paste::paste! {
            #[allow(unused)]
            macro_rules! [<take_ $name>] {
                ($pins:expr) => {
                    $pins.$p
                }
            }

            #[allow(unused)]
            pub(crate) use [<take_ $name>];
        }

        #[allow(unused)]
        pub(crate) use $name;
    };
}

macro_rules! define_pin_lr {
    ($name:ident, $pl:ident, $pr:ident) => {
        #[allow(unused)]
        macro_rules! $name {
                                                            () => {
                                                                #[cfg(feature = "side_left")]
                                                                ::embassy_nrf::peripherals::$pl
                                                                #[cfg(feature = "side_right")]
                                                                ::embassy_nrf::peripherals::$pr
                                                            };
                                                        }

        ::paste::paste! {
            #[allow(unused)]
            #[cfg(feature = "side_left")]
            macro_rules! [<take_ $name>] {
                ($pins:expr) => {
                    $pins.$pl
                }
            }

            #[allow(unused)]
            #[cfg(feature = "side_right")]
            macro_rules! [<take_ $name>] {
                ($pins:expr) => {
                    $pins.$pr
                }
            }

            #[allow(unused)]
            pub(crate) use [<take_ $name>];
        }

        #[allow(unused)]
        pub(crate) use $name;
    };
}

define_pin_lr!(leds, P0_27, P0_13);
define_pin_lr!(leds_pwr, P0_31, P0_19);

define_pin!(row_0, P0_26);
define_pin!(row_1, P0_05);
define_pin_lr!(row_2, P0_06, P0_07);
define_pin_lr!(row_3, P0_08, P1_08);
define_pin_lr!(row_4, P0_07, P0_11);
define_pin_lr!(row_5, P1_09, P0_12);

define_pin_lr!(col_0, P1_08, P1_06);
define_pin!(col_1, P1_04);
define_pin_lr!(col_2, P1_06, P0_02);
define_pin!(col_3, P1_07);
define_pin!(col_4, P1_05);
define_pin!(col_5, P1_03);
define_pin!(col_6, P1_01);
