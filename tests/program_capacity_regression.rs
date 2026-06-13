#![allow(long_running_const_eval)]

use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};

macro_rules! visible_route {
    ($left:literal, $right:literal) => {
        g::route(
            g::send::<0, 1, Msg<$left, ()>>(),
            g::send::<0, 1, Msg<$right, ()>>(),
        )
    };
}

macro_rules! seq_chain {
    ($single:expr $(,)?) => {
        $single
    };
    ($head:expr, $($tail:expr),+ $(,)?) => {
        g::seq($head, seq_chain!($($tail),+))
    };
}

#[test]
fn public_route_program_capacity_exceeds_segment_marker_shortcut() {
    let program = seq_chain!(
        visible_route!(1, 2),
        visible_route!(3, 4),
        visible_route!(5, 6),
        visible_route!(7, 8),
        visible_route!(9, 10),
        visible_route!(11, 12),
        visible_route!(13, 14),
        visible_route!(15, 16),
        visible_route!(17, 18),
        visible_route!(19, 20),
        visible_route!(21, 22),
        visible_route!(23, 24),
        visible_route!(25, 26),
        visible_route!(27, 28),
        visible_route!(29, 30),
        visible_route!(31, 32),
        visible_route!(33, 34),
        visible_route!(35, 36),
        visible_route!(37, 38),
        visible_route!(39, 40),
        visible_route!(41, 42),
        visible_route!(43, 44),
        visible_route!(45, 46),
        visible_route!(47, 48),
        visible_route!(49, 50),
        visible_route!(51, 52),
        visible_route!(53, 54),
        visible_route!(55, 56),
        visible_route!(57, 58),
        visible_route!(59, 60),
        visible_route!(61, 62),
        visible_route!(63, 64),
        visible_route!(65, 66),
    );

    let _: RoleProgram<0> = project(&program);
}
