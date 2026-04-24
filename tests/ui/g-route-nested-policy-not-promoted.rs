use hibana::g::{self, Msg, Role};
use hibana::substrate::program::{RoleProgram, project};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};

const LOOP_POLICY_ID: u16 = 10;
const LABEL_LOOP_CONTINUE: u8 = 48;
const LABEL_LOOP_BREAK: u8 = 49;

fn main() {
    let loop_continue_arm = || {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>()
    };

    let loop_break_arm = || {
        g::send::<
            Role<0>,
            Role<0>,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                LoopBreakKind,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>()
    };

    let loop_program = || g::route(loop_continue_arm(), loop_break_arm());
    let outer_loop_continue_arm = || g::seq(loop_continue_arm(), loop_program());
    let nested_loop_program = g::route(outer_loop_continue_arm(), loop_break_arm());

    let _: RoleProgram<0> = project(&nested_loop_program);
}
