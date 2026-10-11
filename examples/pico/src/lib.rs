#![no_std]

use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
};

/// Project the same compact two-role protocol used by the runnable host example.
#[inline(never)]
pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>) {
    let choreography = g::seq(
        g::send::<0, 1, Msg<1, u32>>(),
        g::send::<1, 0, Msg<2, u32>>(),
    );
    (project(&choreography), project(&choreography))
}
