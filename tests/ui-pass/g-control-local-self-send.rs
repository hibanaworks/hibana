//! Built-in local control events are explicit choreography self-sends.

use hibana::g;
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let program = g::send::<1, 1, g::ControlMsg<1, g::control::LoopContinue>>();
    let role: RoleProgram<1> = project(&program);
    let loop_break = g::send::<1, 1, g::ControlMsg<6, g::control::LoopBreak>>();
    let loop_break_role: RoleProgram<1> = project(&loop_break);
    let snapshot = g::send::<1, 1, g::ControlMsg<2, g::control::StateSnapshot>>();
    let snapshot_role: RoleProgram<1> = project(&snapshot);
    let restore = g::send::<1, 1, g::ControlMsg<3, g::control::StateRestore>>();
    let restore_role: RoleProgram<1> = project(&restore);
    let commit = g::send::<1, 1, g::ControlMsg<4, g::control::TxnCommit>>();
    let commit_role: RoleProgram<1> = project(&commit);
    let abort = g::send::<1, 1, g::ControlMsg<5, g::control::TxnAbort>>();
    let abort_role: RoleProgram<1> = project(&abort);
    let _ = (
        role,
        loop_break_role,
        snapshot_role,
        restore_role,
        commit_role,
        abort_role,
    );
}
