use hibana::g::{self, Msg};
use hibana::integration::cap::control::{LoopBreakKind, LoopContinueKind};
use hibana::integration::program::{RoleProgram, project};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;

fn main() {
    let break_arm = g::send::<
        0,
        0,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>,
        0,
    >();
    let continue_arm = g::send::<
        0,
        0,
        Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
        0,
    >();

    let program = g::route(break_arm, continue_arm);
    let _: RoleProgram<0> = project(&program);
}
