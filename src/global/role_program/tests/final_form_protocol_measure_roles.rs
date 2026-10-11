macro_rules! final_form_protocol_measure_roles {
    (minimal_send_recv, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        measure_role(&role0).max(measure_role(&role1))
    }};
    (nested_par_join, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        ProtocolMatrixMeasurement::EMPTY
            .max(measure_role(&role0))
            .max(measure_role(&role1))
            .max(measure_role(&role2))
            .max(measure_role(&role3))
    }};
    (route_with_unselected_nested_par, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        ProtocolMatrixMeasurement::EMPTY
            .max(measure_role(&role0))
            .max(measure_role(&role1))
            .max(measure_role(&role2))
            .max(measure_role(&role3))
    }};
    (triple_nested_route, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        measure_role(&role0).max(measure_role(&role1))
    }};
    (passive_nested_route_observer, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        ProtocolMatrixMeasurement::EMPTY
            .max(measure_role(&role0))
            .max(measure_role(&role1))
            .max(measure_role(&role2))
            .max(measure_role(&role3))
    }};
    (alternating_par_route, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        let role2: RoleProgram<2> = project($program);
        let role3: RoleProgram<3> = project($program);
        ProtocolMatrixMeasurement::EMPTY
            .max(measure_role(&role0))
            .max(measure_role(&role1))
            .max(measure_role(&role2))
            .max(measure_role(&role3))
    }};
    (huge_legal_choreography, $program:expr) => {{
        let role0: RoleProgram<0> = project($program);
        let role1: RoleProgram<1> = project($program);
        measure_role(&role0).max(measure_role(&role1))
    }};
}
