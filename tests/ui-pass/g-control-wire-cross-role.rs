//! Built-in wire control events are explicit cross-role choreography sends.

use hibana::g;
use hibana::integration::program::{RoleProgram, project};

fn main() {
    let program = g::send::<1, 2, g::ControlMsg<20, g::control::TopologyBegin>>();
    let source: RoleProgram<1> = project(&program);
    let target: RoleProgram<2> = project(&program);
    let ack = g::send::<2, 1, g::ControlMsg<21, g::control::TopologyAck>>();
    let ack_source: RoleProgram<2> = project(&ack);
    let ack_target: RoleProgram<1> = project(&ack);
    let commit = g::send::<1, 2, g::ControlMsg<22, g::control::TopologyCommit>>();
    let commit_source: RoleProgram<1> = project(&commit);
    let commit_target: RoleProgram<2> = project(&commit);
    let _ = (
        source,
        target,
        ack_source,
        ack_target,
        commit_source,
        commit_target,
    );
}
