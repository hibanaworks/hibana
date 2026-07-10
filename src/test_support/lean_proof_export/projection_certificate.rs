use super::*;

fn lean_usize_list(values: impl Iterator<Item = usize>) -> String {
    let body = values
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

fn lean_local_action(action: crate::global::typestate::LocalAction) -> String {
    match action {
        crate::global::typestate::LocalAction::Send { peer, label, .. } => {
            format!(".send {peer} {label}")
        }
        crate::global::typestate::LocalAction::Recv { peer, label, .. } => {
            format!(".recv {peer} {label}")
        }
        crate::global::typestate::LocalAction::Local { label, .. } => {
            format!(".local {label}")
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
        events.push(format!(
            "    {{ action := {} }}",
            lean_local_action(production.event_program.node(index).action())
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
    format!(
        "def {name} : Hibana.ProjectionCertificate := {{\n  role := {ROLE}\n  choreo := {choreo}\n  \
         topology := {topology}\n}}\n\n\
         example : {name}.check = true := by\n  decide\n\n\
         example : {name}.RefinesTopology :=\n  Hibana.projection_certificate_sound (by decide)\n"
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
