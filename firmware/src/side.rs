use shared::side::KeyboardSide;

pub const fn is_this_side(side: KeyboardSide) -> bool {
    // no const eq?
    match get_side() {
        KeyboardSide::Left => matches!(side, KeyboardSide::Left),
        KeyboardSide::Right => matches!(side, KeyboardSide::Right),
    }
}

pub const fn get_side() -> KeyboardSide {
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

pub const fn get_other_side() -> KeyboardSide {
    get_side().other()
}

pub const fn is_master() -> bool {
    is_this_side(KeyboardSide::Right)
}
