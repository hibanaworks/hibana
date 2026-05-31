#![allow(long_running_const_eval)]

use hibana::g::{self, Msg};
use hibana::integration::cap::control::RouteDecisionKind;
use hibana::integration::program::{RoleProgram, project};

macro_rules! policy_route {
    ($left:literal, $right:literal) => {
        g::route(
            g::send::<0, 0, Msg<$left, (), RouteDecisionKind>, 0>().policy::<7>(),
            g::send::<0, 0, Msg<$right, (), RouteDecisionKind>, 0>().policy::<7>(),
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
fn public_policy_program_capacity_exceeds_segment_marker_shortcut() {
    let program = seq_chain!(
        policy_route!(1, 2),
        policy_route!(3, 4),
        policy_route!(5, 6),
        policy_route!(7, 8),
        policy_route!(9, 10),
        policy_route!(11, 12),
        policy_route!(13, 14),
        policy_route!(15, 16),
        policy_route!(17, 18),
        policy_route!(19, 20),
        policy_route!(21, 22),
        policy_route!(23, 24),
        policy_route!(25, 26),
        policy_route!(27, 28),
        policy_route!(29, 30),
        policy_route!(31, 32),
        policy_route!(33, 34),
        policy_route!(35, 36),
        policy_route!(37, 38),
        policy_route!(39, 40),
        policy_route!(41, 42),
        policy_route!(43, 44),
        policy_route!(45, 46),
        policy_route!(47, 48),
        policy_route!(49, 50),
        policy_route!(51, 52),
        policy_route!(53, 54),
        policy_route!(55, 56),
        policy_route!(57, 58),
        policy_route!(59, 60),
        policy_route!(61, 62),
        policy_route!(63, 64),
        policy_route!(65, 66),
    );

    let _: RoleProgram<0> = project(&program);
}
