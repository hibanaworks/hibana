use hibana::g::{self, Msg, Role};
use hibana::integration::program::{RoleProgram, project};

macro_rules! policy_send {
    ($label:literal) => {
        g::send::<Role<0>, Role<1>, Msg<$label, ()>, 0>().policy::<7>()
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
        policy_send!(1),
        policy_send!(2),
        policy_send!(3),
        policy_send!(4),
        policy_send!(5),
        policy_send!(6),
        policy_send!(7),
        policy_send!(8),
        policy_send!(9),
        policy_send!(10),
        policy_send!(11),
        policy_send!(12),
        policy_send!(13),
        policy_send!(14),
        policy_send!(15),
        policy_send!(16),
        policy_send!(17),
        policy_send!(18),
        policy_send!(19),
        policy_send!(20),
        policy_send!(21),
        policy_send!(22),
        policy_send!(23),
        policy_send!(24),
        policy_send!(25),
        policy_send!(26),
        policy_send!(27),
        policy_send!(28),
        policy_send!(29),
        policy_send!(30),
        policy_send!(31),
        policy_send!(32),
        policy_send!(33),
        policy_send!(34),
        policy_send!(35),
        policy_send!(36),
        policy_send!(37),
        policy_send!(38),
        policy_send!(39),
        policy_send!(40),
        policy_send!(41),
        policy_send!(42),
        policy_send!(43),
        policy_send!(44),
        policy_send!(45),
        policy_send!(46),
        policy_send!(47),
        policy_send!(48),
        policy_send!(49),
        policy_send!(50),
        policy_send!(51),
        policy_send!(52),
        policy_send!(53),
        policy_send!(54),
        policy_send!(55),
        policy_send!(56),
        policy_send!(57),
        policy_send!(58),
        policy_send!(59),
        policy_send!(60),
        policy_send!(61),
        policy_send!(62),
        policy_send!(63),
        policy_send!(64),
        policy_send!(65),
    );

    let _: RoleProgram<0> = project(&program);
}
