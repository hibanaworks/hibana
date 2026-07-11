use super::*;

fn lean_usize_list(values: impl Iterator<Item = usize>) -> String {
    let body = values
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

fn lean_byte_list(values: impl Iterator<Item = u8>) -> String {
    let body = values
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

fn lean_local_action(action: crate::global::typestate::LocalAction, payload_schema: u32) -> String {
    match action {
        crate::global::typestate::LocalAction::Send { peer, label, .. } => {
            format!(".send {peer} {label} {payload_schema}")
        }
        crate::global::typestate::LocalAction::Recv { peer, label, .. } => {
            format!(".recv {peer} {label} {payload_schema}")
        }
        crate::global::typestate::LocalAction::Local { label, .. } => {
            format!(".local {label} {payload_schema}")
        }
        crate::global::typestate::LocalAction::Terminate => {
            panic!("production projection certificate cannot contain a terminal event")
        }
    }
}

fn lean_projection_events(production: &ProductionCursorTrace) -> String {
    let mut events = Vec::new();
    let mut index = 0usize;
    while index < production.event_program.local_len() {
        let key = production
            .action_key_at(index)
            .expect("production projection event must retain a message key");
        events.push(format!(
            "    {{ action := {} }}",
            lean_local_action(production.event_program.node(index).action(), key.schema)
        ));
        index += 1;
    }
    format!("[\n{}\n  ]", events.join(",\n"))
}

fn lean_projection_routes(production: &ProductionCursorTrace) -> String {
    let mut routes = Vec::new();
    let mut slot = 0usize;
    while let Some(region) = production.event_program.route_scope_rows_by_slot(slot) {
        let program = production.event_program.program_ref();
        let mut authority_row = None;
        let mut row = 0usize;
        while row < program.route_resolver_row_count() {
            if program.route_resolver_scope_at_row(row) == Some(region.scope()) {
                authority_row = Some((row, program.route_resolver_id_at_row(row)));
                break;
            }
            row += 1;
        }
        let Some((conflict, resolver_id)) = authority_row else {
            panic!("production route scope is missing its global authority row");
        };
        let authority = match resolver_id {
            None => ".intrinsic".to_string(),
            Some(resolver_id) => format!(".dynamic {resolver_id}"),
        };
        let arm_events = |arm| {
            production
                .event_program
                .route_arm_event_row_by_slot(slot, arm)
                .map_or_else(
                    || "[]".to_string(),
                    |row| lean_usize_list(row.start()..row.end()),
                )
        };
        let reentry = if region.reentry() {
            ".rolled"
        } else {
            ".singlePass"
        };
        routes.push(format!(
            "    {{ conflict := {conflict}, authority := {authority}, leftEvents := {}, \
             rightEvents := {}, reentry := {reentry} }}",
            arm_events(0),
            arm_events(1)
        ));
        slot += 1;
    }
    format!("[\n{}\n  ]", routes.join(",\n"))
}

fn lean_projection_rolls(production: &ProductionCursorTrace) -> String {
    let mut rolls = Vec::new();
    let mut slot = 0usize;
    while let Some((_scope, row)) = production.event_program.roll_scope_row_by_slot(slot) {
        rolls.push(format!(
            "    {{ events := {} }}",
            lean_usize_list(row.start()..row.end())
        ));
        slot += 1;
    }
    format!("[\n{}\n  ]", rolls.join(",\n"))
}

pub(super) fn projection_certificate_source<const ROLE: u8>(
    program: &impl crate::global::program::Projectable,
    choreo: &str,
    name: &str,
) -> String {
    let production = ProductionCursorTrace::new::<ROLE>(program);
    let topology = format!(
        "{{\n  events := {},\n  rolls := {},\n  routes := {}\n}}",
        lean_projection_events(&production),
        lean_projection_rolls(&production),
        lean_projection_routes(&production),
    );
    let program = production.event_program.program_ref();
    let descriptor = production.descriptor;
    let resident = descriptor.local_event_rows();
    let footprint = resident.footprint();
    let program_columns = program.columns;
    let role_columns = descriptor.local_event_rows().columns;
    let program_bytes =
        lean_byte_list((0..program.proof_blob_len()).map(|offset| program.proof_byte_at(offset)));
    let role_bytes = lean_byte_list(
        (0..descriptor.proof_blob_len()).map(|offset| descriptor.proof_byte_at(offset)),
    );
    assert_ne!(
        descriptor.local_len(),
        0,
        "exact descriptor proof fixture must contain one lane-bound action"
    );
    let first_lane_offset =
        descriptor.local_len() * crate::global::role_program::ROLE_IMAGE_EVENT_STRIDE;
    let mismatched_first_lane = descriptor.proof_byte_at(first_lane_offset).wrapping_add(1);
    let exact_name = format!("{name}Exact");
    let lane_mismatch_name = format!("{exact_name}LaneMismatch");
    let eff_index_mismatch_name = format!("{exact_name}EffIndexMismatch");
    let frame_label_mismatch_name = format!("{exact_name}FrameLabelMismatch");
    let event_scope_mismatch_name = format!("{exact_name}EventScopeMismatch");
    let event_flag_mismatch_name = format!("{exact_name}EventFlagMismatch");
    let global_action_mismatch_name = format!("{exact_name}GlobalActionMismatch");
    let global_origin_mismatch_name = format!("{exact_name}GlobalOriginMismatch");
    let global_lane_mismatch_name = format!("{exact_name}GlobalLaneMismatch");
    let mismatched_first_global_label = program.proof_byte_at(4).wrapping_add(1);
    let mismatched_first_global_origin = program.proof_byte_at(9) ^ 1;
    let mismatched_first_global_lane = program.proof_byte_at(10).wrapping_add(1);
    let mismatched_first_eff_index = descriptor.proof_byte_at(0).wrapping_add(1);
    let mismatched_first_frame_label = descriptor.proof_byte_at(8).wrapping_add(1);
    let first_event_scope =
        u16::from(descriptor.proof_byte_at(6)) | (u16::from(descriptor.proof_byte_at(7)) << 8);
    let mismatched_first_event_scope = if first_event_scope == u16::MAX {
        0
    } else {
        u16::MAX
    };
    let mismatched_first_event_flag = descriptor.proof_byte_at(9) ^ 1;
    let scope_marker_mismatch_source = if program_columns.scope_marker_count() == 0 {
        String::new()
    } else {
        let mismatch_name = format!("{exact_name}ScopeMarkerMismatch");
        let tag_offset = program_columns.scope_markers().offset as usize + 4;
        let mismatched_tag = program.proof_byte_at(tag_offset) ^ 4;
        format!(
            "\n\ndef {mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
             {exact_name}.image.programBytes.set \
             ({exact_name}.image.programScopeMarkerOffset + 4) {mismatched_tag}\n  }}\n}}\n\n\
             example : {mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let route_mismatch_source = if role_columns.route_scopes.len == 0 {
        String::new()
    } else {
        let route_mismatch_name = format!("{exact_name}RouteMismatch");
        let route_scope_high_byte = role_columns.route_scopes.offset as usize + 1;
        let mismatched_route_scope = descriptor.proof_byte_at(route_scope_high_byte) | 0x80;
        format!(
            "\n\ndef {route_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set ({exact_name}.image.roleRouteScopeOffset + 1) \
             {mismatched_route_scope}\n  }}\n}}\n\n\
             example : {route_mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let dependency_mismatch_source = if role_columns.dependencies.len == 0 {
        String::new()
    } else {
        let scope_mismatch_name = format!("{exact_name}DependencyScopeMismatch");
        let conflict_mismatch_name = format!("{exact_name}DependencyConflictMismatch");
        let scope_offset = role_columns.dependencies.offset as usize + 4;
        let conflict_offset = role_columns.dependencies.offset as usize + 6;
        let mismatched_scope = descriptor.proof_byte_at(scope_offset) ^ 1;
        let mismatched_conflict = descriptor.proof_byte_at(conflict_offset) ^ 1;
        format!(
            "\n\ndef {scope_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set \
             ({exact_name}.image.roleDependencyOffset + 4) {mismatched_scope}\n  }}\n}}\n\n\
             example : {scope_mismatch_name}.check = false := by\n  native_decide\n\n\
             def {conflict_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set \
             ({exact_name}.image.roleDependencyOffset + 6) {mismatched_conflict}\n  }}\n}}\n\n\
             example : {conflict_mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let metadata_mismatch_name = format!("{exact_name}LogicalLaneCountMismatch");
    let mismatched_logical_lane_count = if descriptor.logical_lane_count() < 256 {
        descriptor.logical_lane_count() + 1
    } else {
        descriptor.logical_lane_count() - 1
    };
    let lane_bit_mismatch_name = format!("{exact_name}LaneBitMismatch");
    assert_ne!(role_columns.lane_bits.len, 0);
    let mismatched_lane_bit = descriptor.proof_byte_at(role_columns.lane_bits.offset as usize) ^ 1;
    let resident_mismatch_source = format!(
        "\n\ndef {metadata_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    logicalLaneCount := \
         {mismatched_logical_lane_count}\n  }}\n}}\n\n\
         example : {metadata_mismatch_name}.check = false := by\n  native_decide\n\n\
         def {lane_bit_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         {exact_name}.image.roleBytes.set {exact_name}.image.roleLaneBitOffset \
         {mismatched_lane_bit}\n  }}\n}}\n\n\
         example : {lane_bit_mismatch_name}.check = false := by\n  native_decide"
    );
    let lane_step_mismatch_source =
        if role_columns.route_arm_lane_step_rows.len == 0 || descriptor.logical_lane_count() < 2 {
            String::new()
        } else {
            let mismatch_name = format!("{exact_name}RouteArmLaneStepMismatch");
            let offset = role_columns.route_arm_lane_step_rows.offset as usize;
            let lane = descriptor.proof_byte_at(offset);
            let mismatched_lane = (usize::from(lane) + 1) % descriptor.logical_lane_count();
            assert_ne!(usize::from(lane), mismatched_lane);
            format!(
                "\n\ndef {mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set {exact_name}.image.roleRouteArmLaneStepOffset \
             {mismatched_lane}\n  }}\n}}\n\n\
             example : {mismatch_name}.check = false := by\n  native_decide"
            )
        };
    let route_child_mismatch_source = if role_columns.route_scopes.len < 2 {
        String::new()
    } else {
        let mismatch_name = format!("{exact_name}RouteChildMismatch");
        let offset = role_columns.route_arms.offset as usize + 3;
        let child = descriptor.proof_byte_at(offset);
        let mismatched_child = if child == 0 { 1 } else { 0 };
        format!(
            "\n\ndef {mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set ({exact_name}.image.roleRouteArmOffset + 3) \
             {mismatched_child}\n  }}\n}}\n\n\
             example : {mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let route_commit_mismatch_source = if role_columns.route_commit_rows.len == 0 {
        String::new()
    } else {
        let mismatch_name = format!("{exact_name}RouteCommitMismatch");
        let offset = role_columns.route_commit_rows.offset as usize;
        let mismatched_commit = descriptor.proof_byte_at(offset) ^ 1;
        format!(
            "\n\ndef {mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set {exact_name}.image.roleRouteCommitRowOffset \
             {mismatched_commit}\n  }}\n}}\n\n\
             example : {mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let resolver_mismatch_source = if program_columns.route_resolver_count() == 0 {
        String::new()
    } else {
        let resolver_mismatch_name = format!("{exact_name}ResolverMismatch");
        let resolver_low_byte = program_columns.route_resolvers().offset as usize + 2;
        let mismatched_resolver = program.proof_byte_at(resolver_low_byte) ^ 1;
        let controller_mismatch_name = format!("{exact_name}ControllerMismatch");
        let controller_offset = program_columns.route_resolvers().offset as usize + 4;
        let controller = program.proof_byte_at(controller_offset);
        let mismatched_controller = if controller == u8::MAX {
            0
        } else {
            (controller + 1) % program.role_count() as u8
        };
        assert_ne!(controller, mismatched_controller);
        format!(
            "\n\ndef {resolver_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
             {exact_name}.image.programBytes.set \
             ({exact_name}.image.programRouteResolverOffset + 2) {mismatched_resolver}\n  }}\n}}\n\n\
             example : {resolver_mismatch_name}.check = false := by\n  native_decide\n\n\
             def {controller_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
             {exact_name}.image.programBytes.set \
             ({exact_name}.image.programRouteResolverOffset + 4) {mismatched_controller}\n  }}\n}}\n\n\
             example : {controller_mismatch_name}.check = false := by\n  native_decide"
        )
    };
    let roll_mismatch_source = if role_columns.roll_scopes.len == 0 {
        String::new()
    } else {
        let roll_mismatch_name = format!("{exact_name}RollMismatch");
        let roll_scope_low_byte = role_columns.roll_scopes.offset as usize;
        let mismatched_roll_scope = descriptor.proof_byte_at(roll_scope_low_byte) ^ 1;
        format!(
            "\n\ndef {roll_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
             {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
             {exact_name}.image.roleBytes.set {exact_name}.image.roleRollScopeOffset \
             {mismatched_roll_scope}\n  }}\n}}\n\n\
             example : {roll_mismatch_name}.check = false := by\n  native_decide"
        )
    };
    format!(
        "def {name} : Hibana.ProjectionCertificate := {{\n  role := {ROLE}\n  choreo := {choreo}\n  \
         topology := {topology}\n}}\n\n\
         example : {name}.check = true := by\n  decide\n\n\
         example : {name}.RefinesTopology :=\n  Hibana.projection_certificate_sound (by decide)\n\n\
         def {exact_name} : Hibana.ExactDescriptorCertificate := {{\n  image := {{\n    \
         roleCount := {}\n    role := {ROLE}\n    logicalLaneCount := {}\n    activeLaneCount := {}\n    \
         endpointLaneSlotCount := {}\n    maxRouteStackDepth := {}\n    firstActiveLane := {}\n    \
         activeLaneStart := {}\n    activeLaneLength := {}\n    atomCount := {}\n    routeResolverCount := {}\n    \
         scopeMarkerCount := {}\n    eventCount := {}\n    dependencyRowCount := {}\n    \
         conflictRowCount := {}\n    routeScopeCount := {}\n    residentBoundaryCount := {}\n    \
         laneBitCount := {}\n    routeArmLaneStepCount := {}\n    routeCommitRowCount := {}\n    \
         rollScopeCount := {}\n    \
         programBytes := {program_bytes}\n    roleBytes := {role_bytes}\n  }}\n  choreo := {choreo}\n}}\n\n\
         example : {exact_name}.check = true := by\n  native_decide\n\n\
         example : {exact_name}.Refines :=\n  \
         Hibana.exact_descriptor_certificate_sound (by native_decide)\n\n\
         def {lane_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         {exact_name}.image.roleBytes.set {exact_name}.image.roleLaneOffset \
         {mismatched_first_lane}\n  }}\n}}\n\n\
         example : {lane_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {eff_index_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         {exact_name}.image.roleBytes.set 0 {mismatched_first_eff_index}\n  }}\n}}\n\n\
         example : {eff_index_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {frame_label_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         {exact_name}.image.roleBytes.set 8 {mismatched_first_frame_label}\n  }}\n}}\n\n\
         example : {frame_label_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {event_scope_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         ({exact_name}.image.roleBytes.set 6 {}).set 7 {}\n  }}\n}}\n\n\
         example : {event_scope_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {event_flag_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    roleBytes := \
         {exact_name}.image.roleBytes.set 9 {mismatched_first_event_flag}\n  }}\n}}\n\n\
         example : {event_flag_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {global_action_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
         {exact_name}.image.programBytes.set 4 {mismatched_first_global_label}\n  }}\n}}\n\n\
         example : {global_action_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {global_origin_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
         {exact_name}.image.programBytes.set 9 {mismatched_first_global_origin}\n  }}\n}}\n\n\
         example : {global_origin_mismatch_name}.check = false := by\n  native_decide\
         \n\ndef {global_lane_mismatch_name} : Hibana.ExactDescriptorCertificate := {{\n  \
         {exact_name} with\n  image := {{ {exact_name}.image with\n    programBytes := \
         {exact_name}.image.programBytes.set 10 {mismatched_first_global_lane}\n  }}\n}}\n\n\
         example : {global_lane_mismatch_name}.check = false := by\n  native_decide\
         {scope_marker_mismatch_source}{route_mismatch_source}{dependency_mismatch_source}{resident_mismatch_source}{lane_step_mismatch_source}{route_child_mismatch_source}{route_commit_mismatch_source}{resolver_mismatch_source}{roll_mismatch_source}\n",
        program.role_count(),
        descriptor.logical_lane_count(),
        footprint.active_lane_count,
        descriptor.endpoint_lane_slot_count(),
        descriptor.max_route_stack_depth(),
        descriptor.first_active_lane().unwrap_or(u16::MAX as usize),
        resident.active_lane_row.start(),
        resident.active_lane_row.len(),
        program.proof_atom_count(),
        program_columns.route_resolver_count(),
        program_columns.scope_marker_count(),
        descriptor.local_len(),
        role_columns.dependencies.len,
        role_columns.conflicts.len,
        role_columns.route_scopes.len,
        role_columns.resident_boundaries.len,
        role_columns.lane_bits.len,
        role_columns.route_arm_lane_step_rows.len,
        role_columns.route_commit_rows.len,
        role_columns.roll_scopes.len,
        mismatched_first_event_scope as u8,
        (mismatched_first_event_scope >> 8) as u8,
    )
}

pub(super) fn progress_certificate_source(choreo: &str, role: u8, name: &str) -> String {
    format!(
        "def {name} : Hibana.ProgressCertificate :=\n  \
         Hibana.buildProgressCertificate (Hibana.projectGraph {role} {choreo})\n\n\
         example : {name}.check (Hibana.projectGraph {role} {choreo}) = true := by\n  decide\n\n\
         example {{state : Hibana.CompactCommitState}}\n    \
         (reachable : Hibana.CompactReachable (Hibana.projectGraph {role} {choreo}) state) :\n    \
         Hibana.LogicalProgress (Hibana.projectGraph {role} {choreo}) state :=\n  \
         Hibana.reachable_state_has_logical_progress (certificate := {name}) (by decide) reachable\n"
    )
}
