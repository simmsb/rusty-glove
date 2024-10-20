use shared::side::KeyboardSide;

pub fn is_this_side(side: KeyboardSide) -> bool {
    get_side() == side
}

pub fn get_side() -> KeyboardSide {
    #[cfg(feature = "side_left")]
    {
        return KeyboardSide::Left;
    }
    #[cfg(feature = "side_right")]
    {
        KeyboardSide::Right
    }
    #[cfg(any(
        not(any(feature = "side_left", feature = "side_right")),
        all(feature = "side_left", feature = "side_right")
    ))]
    compile_error!("Select feature side_left or side_right")
}

pub fn get_other_side() -> KeyboardSide {
    get_side().other()
}

pub fn is_master() -> bool {
    is_this_side(KeyboardSide::Right)
}
