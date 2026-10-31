//! Balanced protocol repetition shared by native timing and work-count fixtures.
macro_rules! repeated {
    ($term:expr; 1) => { $term };
    ($term:expr; 2) => { $crate::g::seq(repeated!($term; 1), repeated!($term; 1)) };
    ($term:expr; 4) => { $crate::g::seq(repeated!($term; 2), repeated!($term; 2)) };
    ($term:expr; 8) => { $crate::g::seq(repeated!($term; 4), repeated!($term; 4)) };
    ($term:expr; 16) => { $crate::g::seq(repeated!($term; 8), repeated!($term; 8)) };
    ($term:expr; 32) => { $crate::g::seq(repeated!($term; 16), repeated!($term; 16)) };
    ($term:expr; 64) => { $crate::g::seq(repeated!($term; 32), repeated!($term; 32)) };
    ($term:expr; 128) => { $crate::g::seq(repeated!($term; 64), repeated!($term; 64)) };
    ($term:expr; 256) => { $crate::g::seq(repeated!($term; 128), repeated!($term; 128)) };
}
pub(crate) use repeated;
