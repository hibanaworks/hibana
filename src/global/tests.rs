use super::{ControlDesc, Program, role_program::RoleProgram};
use core::mem::size_of;

#[test]
fn descriptor_first_size_gates_hold() {
    assert_eq!(
        size_of::<Program<()>>(),
        0,
        "Program<Steps> must stay zero-sized"
    );
    assert!(
        size_of::<RoleProgram<0>>() <= 24,
        "RoleProgram<ROLE> must stay compact"
    );
    assert!(
        size_of::<ControlDesc>() <= 16,
        "ControlDesc must stay within the packed descriptor budget"
    );
}
