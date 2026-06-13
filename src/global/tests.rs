use super::role_program::RoleProgram;
use crate::g::Program;
use crate::global::compiled::images::PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE;
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
        PROGRAM_IMAGE_ROUTE_RESOLVER_STRIDE <= 12,
        "route resolver rows must stay within the packed program image budget"
    );
}
