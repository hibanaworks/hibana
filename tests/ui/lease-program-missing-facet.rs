#![allow(dead_code)]

use hibana::g::{self, StepNil};

const PROGRAM: g::Program<StepNil> = g::Program::empty();

static ROLE_PROGRAM: g::RoleProgram<'static, 0, StepNil> =
    g::project::<0, StepNil, _>(&PROGRAM);

const NEEDS: hibana::control::lease::planner::LeaseFacetNeeds =
    hibana::control::lease::planner::LeaseFacetNeeds::new().with_caps();

const _: () =
    hibana::control::lease::planner::assert_program_covers_facets(&ROLE_PROGRAM, NEEDS);

fn main() {}
