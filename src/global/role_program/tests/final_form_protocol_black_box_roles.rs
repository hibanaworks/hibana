macro_rules! final_form_protocol_black_box_roles {
    (minimal_send_recv, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        core::hint::black_box((role0, role1));
    }};
    (nested_par_join, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        core::hint::black_box((role0, role1, role2, role3));
    }};
    (route_with_unselected_nested_par, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        core::hint::black_box((role0, role1, role2, role3));
    }};
    (triple_nested_route, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        core::hint::black_box((role0, role1));
    }};
    (passive_nested_route_observer, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        core::hint::black_box((role0, role1, role2, role3));
    }};
    (alternating_par_route, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        core::hint::black_box((role0, role1, role2, role3));
    }};
    (huge_legal_choreography, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        core::hint::black_box((role0, role1));
    }};
}
