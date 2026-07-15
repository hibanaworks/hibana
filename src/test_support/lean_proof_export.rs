use super::{ProductionCursorTrace, selected_arm};
use crate::{g, global::const_dsl::ScopeId};
use std::{
    format, fs,
    path::PathBuf,
    println,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[path = "lean_proof_export/choreo_source.rs"]
mod choreo_source;
#[path = "lean_proof_export/cyclic_roll_certificate.rs"]
mod cyclic_roll_certificate;
#[path = "lean_proof_export/production_kernel_artifact.rs"]
mod production_kernel_artifact;
#[path = "lean_proof_export/projection_certificate.rs"]
mod projection_certificate;
use choreo_source::LeanChoreo;
use projection_certificate::{
    progress_certificate_source, projectability_certificate_source, projection_certificate_source,
    verified_protocol_certificate_source,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ProofKey {
    label: u8,
    schema: u32,
}

impl ProofKey {
    fn lean_source(self) -> String {
        format!("{{ label := {}, schema := {} }}", self.label, self.schema)
    }
}

fn lean_key_list(values: &[ProofKey]) -> String {
    let body = values
        .iter()
        .map(|key| key.lean_source())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{body}]")
}

#[derive(Clone, Copy)]
enum ProofArm {
    Left,
    Right,
}

impl ProofArm {
    const fn index(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    const fn lean_source(self) -> &'static str {
        match self {
            Self::Left => ".left",
            Self::Right => ".right",
        }
    }
}

#[derive(Clone, Copy)]
enum ProofAction {
    Commit(ProofKey),
    Resolve {
        conflict: u16,
        resolver: u16,
        arm: ProofArm,
    },
    Reject {
        conflict: u16,
        resolver: u16,
    },
    Stop,
}

#[derive(Clone, Copy)]
enum ProductionStep {
    Commit(u8),
    Resolve {
        conflict: u16,
        resolver: u16,
        arm: ProofArm,
    },
    Reject {
        conflict: u16,
        resolver: u16,
    },
}

impl ProofAction {
    fn lean_source(self) -> String {
        match self {
            Self::Commit(key) => format!(".commit {}", key.lean_source()),
            Self::Resolve {
                conflict,
                resolver,
                arm,
            } => format!(".resolve {conflict} {resolver} {}", arm.lean_source()),
            Self::Reject { conflict, resolver } => {
                format!(".reject {conflict} {resolver}")
            }
            Self::Stop => ".stop".to_string(),
        }
    }
}

fn frame_source(enabled: &[ProofKey], action: ProofAction) -> String {
    format!(
        "  {{ enabled := {}, action := {} }}",
        lean_key_list(enabled),
        action.lean_source()
    )
}

fn unique_enabled_key(enabled: &[ProofKey], label: u8) -> ProofKey {
    let mut matches = enabled.iter().copied().filter(|key| key.label == label);
    let key = matches
        .next()
        .expect("Lean proof fixture label must name an enabled message contract");
    assert!(
        matches.all(|candidate| candidate == key),
        "Lean proof fixture label must not hide distinct enabled payload schemas"
    );
    key
}

fn record_trace<const ROLE: u8>(
    program: &impl crate::global::program::Projectable,
    commits: &[u8],
) -> Vec<(Vec<ProofKey>, ProofAction)> {
    let mut production = ProductionCursorTrace::new::<ROLE>(program);
    let mut frames = Vec::new();
    for &label in commits {
        let mut enabled = production.enabled_keys();
        enabled.sort_unstable();
        let key = unique_enabled_key(&enabled, label);
        assert!(
            enabled.contains(&key),
            "Lean proof fixture attempted disabled label {label}; enabled={enabled:?}"
        );
        frames.push((enabled, ProofAction::Commit(key)));
        production.commit_label(label);
    }
    let mut enabled = production.enabled_keys();
    enabled.sort_unstable();
    frames.push((enabled, ProofAction::Stop));
    frames
}

impl ProductionCursorTrace {
    fn action_key_at(&self, index: usize) -> Option<ProofKey> {
        let (eff_index, label) = match self.event_program.node(index).action() {
            crate::global::typestate::LocalAction::Send {
                eff_index, label, ..
            }
            | crate::global::typestate::LocalAction::Recv {
                eff_index, label, ..
            }
            | crate::global::typestate::LocalAction::Local {
                eff_index, label, ..
            } => (eff_index, label),
            crate::global::typestate::LocalAction::Terminate => return None,
        };
        let atom = self
            .event_program
            .program_ref()
            .atom_at(eff_index.dense_ordinal())
            .expect("production event must retain its global atom");
        Some(ProofKey {
            label,
            schema: atom.payload_schema,
        })
    }

    fn enabled_keys(&self) -> Vec<ProofKey> {
        let mut keys = Vec::new();
        let mut index = 0usize;
        while index < self.descriptor.local_len() {
            if self.enabled_commit_at(index).is_some() {
                keys.push(
                    self.action_key_at(index)
                        .expect("enabled production event must have a message key"),
                );
            }
            index += 1;
        }
        keys
    }

    fn proof_dynamic_scope(&self, conflict: u16, expected_resolver: u16) -> ScopeId {
        let scope = self
            .event_program
            .route_scope_rows_by_slot(conflict as usize)
            .expect("Lean proof conflict must name a production route slot")
            .scope();
        let resolver = self
            .cursor()
            .route_scope_resolver(scope)
            .expect("Lean proof resolver site must exist in the production descriptor");
        assert_eq!(resolver.scope(), scope, "resolver scope metadata diverged");
        assert_eq!(
            resolver.resolver_id(),
            expected_resolver,
            "resolver id metadata diverged"
        );
        scope
    }

    fn apply_proof_resolver_selection(&mut self, conflict: u16, resolver: u16, arm: ProofArm) {
        let scope = self.proof_dynamic_scope(conflict, resolver);
        if let Some(selected_idx) = self
            .selected
            .iter()
            .position(|(candidate, _)| *candidate == scope)
        {
            let old_arm = self.selected[selected_idx].1;
            let mut selected_arm_for_scope = |candidate| selected_arm(&self.selected, candidate);
            assert!(
                self.cursor().reentrant_route_arm_event_row_done(
                    scope,
                    old_arm,
                    &mut selected_arm_for_scope
                ),
                "resolver reentry requires the previous production arm to be complete"
            );
            self.selected.remove(selected_idx);
            self.cursor_mut().clear_reentry_scope_events(scope);
        }
        self.record_or_replace_selected_arm(scope, arm.index());
        assert!(
            !self.enabled_keys().is_empty(),
            "resolver selection must expose a production frontier"
        );
    }

    fn validate_proof_resolver_reject(&self, conflict: u16, resolver: u16) {
        let scope = self.proof_dynamic_scope(conflict, resolver);
        assert!(
            self.selected
                .iter()
                .all(|(candidate, _)| *candidate != scope),
            "resolver reject cannot follow published route authority"
        );
    }
}

fn record_production_steps<const ROLE: u8>(
    program: &impl crate::global::program::Projectable,
    steps: &[ProductionStep],
) -> Vec<(Vec<ProofKey>, ProofAction)> {
    let mut production = ProductionCursorTrace::new::<ROLE>(program);
    let mut frames = Vec::new();
    for (index, &step) in steps.iter().enumerate() {
        let mut enabled = production.enabled_keys();
        enabled.sort_unstable();
        match step {
            ProductionStep::Commit(label) => {
                let key = unique_enabled_key(&enabled, label);
                assert!(
                    enabled.contains(&key),
                    "Lean resolver fixture attempted disabled label {label}; enabled={enabled:?}"
                );
                frames.push((enabled, ProofAction::Commit(key)));
                production.commit_label(label);
            }
            ProductionStep::Resolve {
                conflict,
                resolver,
                arm,
            } => {
                frames.push((
                    enabled,
                    ProofAction::Resolve {
                        conflict,
                        resolver,
                        arm,
                    },
                ));
                production.apply_proof_resolver_selection(conflict, resolver, arm);
            }
            ProductionStep::Reject { conflict, resolver } => {
                frames.push((enabled, ProofAction::Reject { conflict, resolver }));
                assert_eq!(
                    index + 1,
                    steps.len(),
                    "resolver rejection must terminate the production proof trace"
                );
                production.validate_proof_resolver_reject(conflict, resolver);
                return frames;
            }
        }
    }
    let mut enabled = production.enabled_keys();
    enabled.sort_unstable();
    frames.push((enabled, ProofAction::Stop));
    frames
}

fn trace_proof_source(
    choreo: &str,
    name: &str,
    role: u8,
    frames: &[(Vec<ProofKey>, ProofAction)],
) -> String {
    let frame_source = frames
        .iter()
        .map(|(enabled, action)| frame_source(enabled, *action))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "def {name} : List Hibana.TraceFrame := [\n{frame_source}\n]\n\n\
         example : Hibana.checkProgramTrace {choreo} {role} {name} = true := by\n  decide\n\n\
         example :\n    (Hibana.projectGraph {role} {choreo}).WellFormed /\\\n      Hibana.ValidTrace (Hibana.projectGraph {role} {choreo}) .initial {name} :=\n  Hibana.program_trace_checker_sound {choreo} {role} {name} (by decide)\n"
    )
}

#[test]
#[ignore = "host-only Lean proof artifact export"]
fn export_production_trace_for_lean() {
    const RESOLVED_ROUTE: u16 = 901;
    const NESTED_OUTER_RESOLVER: u16 = 902;
    const NESTED_INNER_RESOLVER: u16 = 903;
    const ROLLED_RESOLVER: u16 = 904;
    const REJECTING_RESOLVER: u16 = 905;
    const FULL_ROLE_DOMAIN_RESOLVER: u16 = 906;

    type A = g::Send<0, 1, g::Msg<11, u32>>;
    type B = g::Send<0, 2, g::Msg<12, i32>>;
    type LeftHead = g::Send<0, 1, g::Msg<21, ()>>;
    type LeftTail = g::Send<1, 0, g::Msg<22, ()>>;
    type Left = g::Seq<LeftHead, LeftTail>;
    type Right = g::Send<0, 1, g::Msg<23, ()>>;
    type Choice = g::Route<Left, Right>;
    type Post = g::Send<0, 3, g::Msg<31, ()>>;
    type Steps = g::Seq<g::Par<A, B>, g::Seq<Choice, Post>>;
    type RollLeft = g::Send<0, 1, g::Msg<41, ()>>;
    type RollRight = g::Send<0, 1, g::Msg<42, ()>>;
    type RollPost = g::Send<0, 1, g::Msg<43, ()>>;
    type RolledSteps = g::Roll<g::Seq<g::Route<RollLeft, RollRight>, RollPost>>;
    type NestedHead = g::Send<0, 1, g::Msg<71, ()>>;
    type NestedInnerTail = g::Send<0, 1, g::Msg<72, ()>>;
    type NestedOuterTail = g::Send<0, 1, g::Msg<73, ()>>;
    type NestedRolledSteps =
        g::Roll<g::Seq<g::Roll<g::Seq<NestedHead, NestedInnerTail>>, NestedOuterTail>>;
    type ResolvedLeft = g::Send<0, 1, g::Msg<51, u32>>;
    type ResolvedRight = g::Send<0, 1, g::Msg<51, i32>>;
    type ResolvedSteps = g::Resolve<g::Route<ResolvedLeft, ResolvedRight>, RESOLVED_ROUTE>;
    type NestedResolvedPrefix = g::Send<0, 1, g::Msg<61, ()>>;
    type NestedResolvedLeft = g::Send<0, 1, g::Msg<62, ()>>;
    type NestedResolvedRight = g::Send<0, 1, g::Msg<63, ()>>;
    type NestedResolvedTail = g::Send<0, 1, g::Msg<64, ()>>;
    type NestedResolvedInner =
        g::Resolve<g::Route<NestedResolvedLeft, NestedResolvedRight>, NESTED_INNER_RESOLVER>;
    type NestedResolvedOuterLeft = g::Seq<NestedResolvedPrefix, NestedResolvedInner>;
    type NestedResolvedSteps =
        g::Resolve<g::Route<NestedResolvedOuterLeft, NestedResolvedTail>, NESTED_OUTER_RESOLVER>;
    type RolledResolvedLeft = g::Send<0, 1, g::Msg<81, ()>>;
    type RolledResolvedRight = g::Send<0, 1, g::Msg<82, ()>>;
    type RolledResolvedSteps =
        g::Roll<g::Resolve<g::Route<RolledResolvedLeft, RolledResolvedRight>, ROLLED_RESOLVER>>;
    type RejectLeft = g::Send<0, 1, g::Msg<91, ()>>;
    type RejectRight = g::Send<0, 1, g::Msg<92, ()>>;
    type RejectSteps = g::Resolve<g::Route<RejectLeft, RejectRight>, REJECTING_RESOLVER>;
    type FullRoleDomainLeft = g::Send<254, 255, g::Msg<101, u32>>;
    type FullRoleDomainRight = g::Send<254, 255, g::Msg<102, u32>>;
    type FullRoleDomainSteps = g::Roll<
        g::Resolve<g::Route<FullRoleDomainLeft, FullRoleDomainRight>, FULL_ROLE_DOMAIN_RESOLVER>,
    >;
    type MatchingLeftZero = g::Send<0, 1, g::Msg<111, ()>>;
    type MatchingLeftOne = g::Send<0, 2, g::Msg<112, ()>>;
    type MatchingRightZero = g::Send<3, 4, g::Msg<113, ()>>;
    type MatchingRightOne = g::Send<2, 3, g::Msg<114, ()>>;
    type MatchingSteps = g::Par<
        g::Par<MatchingLeftZero, MatchingLeftOne>,
        g::Par<MatchingRightZero, MatchingRightOne>,
    >;

    let program = g::seq(
        g::par(
            g::send::<0, 1, g::Msg<11, u32>>(),
            g::send::<0, 2, g::Msg<12, i32>>(),
        ),
        g::seq(
            g::route(
                g::seq(
                    g::send::<0, 1, g::Msg<21, ()>>(),
                    g::send::<1, 0, g::Msg<22, ()>>(),
                ),
                g::send::<0, 1, g::Msg<23, ()>>(),
            ),
            g::send::<0, 3, g::Msg<31, ()>>(),
        ),
    );
    let role0 = record_trace::<0>(&program, &[12, 11, 21, 22, 31]);
    let role1 = record_trace::<1>(&program, &[11, 21, 22]);
    let role2 = record_trace::<2>(&program, &[12]);
    let role3 = record_trace::<3>(&program, &[31]);
    let rolled = g::seq(
        g::route(
            g::send::<0, 1, g::Msg<41, ()>>(),
            g::send::<0, 1, g::Msg<42, ()>>(),
        ),
        g::send::<0, 1, g::Msg<43, ()>>(),
    )
    .roll();
    let rolled_left_role0 = record_trace::<0>(&rolled, &[41, 43, 41, 43]);
    let rolled_right_role0 = record_trace::<0>(&rolled, &[42, 43, 42, 43]);
    let rolled_left_role1 = record_trace::<1>(&rolled, &[41, 43, 41, 43]);
    let nested_rolled = g::seq(
        g::seq(
            g::send::<0, 1, g::Msg<71, ()>>(),
            g::send::<0, 1, g::Msg<72, ()>>(),
        )
        .roll(),
        g::send::<0, 1, g::Msg<73, ()>>(),
    )
    .roll();
    let resolved = g::route(
        g::send::<0, 1, g::Msg<51, u32>>(),
        g::send::<0, 1, g::Msg<51, i32>>(),
    )
    .resolve::<RESOLVED_ROUTE>();
    let nested_resolved = g::route(
        g::seq(
            g::send::<0, 1, g::Msg<61, ()>>(),
            g::route(
                g::send::<0, 1, g::Msg<62, ()>>(),
                g::send::<0, 1, g::Msg<63, ()>>(),
            )
            .resolve::<NESTED_INNER_RESOLVER>(),
        ),
        g::send::<0, 1, g::Msg<64, ()>>(),
    )
    .resolve::<NESTED_OUTER_RESOLVER>();
    let rolled_resolved = g::route(
        g::send::<0, 1, g::Msg<81, ()>>(),
        g::send::<0, 1, g::Msg<82, ()>>(),
    )
    .resolve::<ROLLED_RESOLVER>()
    .roll();
    let rejecting = g::route(
        g::send::<0, 1, g::Msg<91, ()>>(),
        g::send::<0, 1, g::Msg<92, ()>>(),
    )
    .resolve::<REJECTING_RESOLVER>();
    let full_role_domain = g::route(
        g::send::<254, 255, g::Msg<101, u32>>(),
        g::send::<254, 255, g::Msg<102, u32>>(),
    )
    .resolve::<FULL_ROLE_DOMAIN_RESOLVER>()
    .roll();
    let cyclic_roll = cyclic_roll_certificate::program();
    let lane_matching = g::par(
        g::par(
            g::send::<0, 1, g::Msg<111, ()>>(),
            g::send::<0, 2, g::Msg<112, ()>>(),
        ),
        g::par(
            g::send::<3, 4, g::Msg<113, ()>>(),
            g::send::<2, 3, g::Msg<114, ()>>(),
        ),
    );
    let nested_rolled_role0 = record_trace::<0>(&nested_rolled, &[71, 72, 73, 71, 72, 73]);
    let resolved_left_role0 = record_production_steps::<0>(
        &resolved,
        &[
            ProductionStep::Resolve {
                conflict: 0,
                resolver: RESOLVED_ROUTE,
                arm: ProofArm::Left,
            },
            ProductionStep::Commit(51),
        ],
    );
    let resolved_right_role0 = record_production_steps::<0>(
        &resolved,
        &[
            ProductionStep::Resolve {
                conflict: 0,
                resolver: RESOLVED_ROUTE,
                arm: ProofArm::Right,
            },
            ProductionStep::Commit(51),
        ],
    );
    let nested_resolved_role0 = record_production_steps::<0>(
        &nested_resolved,
        &[
            ProductionStep::Resolve {
                conflict: 0,
                resolver: NESTED_OUTER_RESOLVER,
                arm: ProofArm::Left,
            },
            ProductionStep::Commit(61),
            ProductionStep::Resolve {
                conflict: 1,
                resolver: NESTED_INNER_RESOLVER,
                arm: ProofArm::Right,
            },
            ProductionStep::Commit(63),
        ],
    );
    let rolled_resolved_role0 = record_production_steps::<0>(
        &rolled_resolved,
        &[
            ProductionStep::Resolve {
                conflict: 0,
                resolver: ROLLED_RESOLVER,
                arm: ProofArm::Left,
            },
            ProductionStep::Commit(81),
            ProductionStep::Resolve {
                conflict: 0,
                resolver: ROLLED_RESOLVER,
                arm: ProofArm::Right,
            },
            ProductionStep::Commit(82),
            ProductionStep::Resolve {
                conflict: 0,
                resolver: ROLLED_RESOLVER,
                arm: ProofArm::Left,
            },
            ProductionStep::Commit(81),
        ],
    );
    let rejected_role0 = record_production_steps::<0>(
        &rejecting,
        &[ProductionStep::Reject {
            conflict: 0,
            resolver: REJECTING_RESOLVER,
        }],
    );
    let cyclic_roll_role2 = cyclic_roll_certificate::trace(&cyclic_roll);
    let total_frames = role0.len()
        + role1.len()
        + role2.len()
        + role3.len()
        + rolled_left_role0.len()
        + rolled_right_role0.len()
        + rolled_left_role1.len()
        + nested_rolled_role0.len()
        + resolved_left_role0.len()
        + resolved_right_role0.len()
        + nested_resolved_role0.len()
        + rolled_resolved_role0.len()
        + rejected_role0.len()
        + cyclic_roll_role2.len();
    let trace_sources = [
        trace_proof_source("generatedChoreo", "generatedTraceRole0", 0, &role0),
        trace_proof_source("generatedChoreo", "generatedTraceRole1", 1, &role1),
        trace_proof_source("generatedChoreo", "generatedTraceRole2", 2, &role2),
        trace_proof_source("generatedChoreo", "generatedTraceRole3", 3, &role3),
        trace_proof_source(
            "generatedRolledChoreo",
            "generatedRolledLeftTraceRole0",
            0,
            &rolled_left_role0,
        ),
        trace_proof_source(
            "generatedRolledChoreo",
            "generatedRolledRightTraceRole0",
            0,
            &rolled_right_role0,
        ),
        trace_proof_source(
            "generatedRolledChoreo",
            "generatedRolledLeftTraceRole1",
            1,
            &rolled_left_role1,
        ),
        trace_proof_source(
            "generatedNestedRolledChoreo",
            "generatedNestedRolledTraceRole0",
            0,
            &nested_rolled_role0,
        ),
        trace_proof_source(
            "generatedResolvedChoreo",
            "generatedResolvedLeftTraceRole0",
            0,
            &resolved_left_role0,
        ),
        trace_proof_source(
            "generatedResolvedChoreo",
            "generatedResolvedRightTraceRole0",
            0,
            &resolved_right_role0,
        ),
        trace_proof_source(
            "generatedNestedResolvedChoreo",
            "generatedNestedResolvedTraceRole0",
            0,
            &nested_resolved_role0,
        ),
        trace_proof_source(
            "generatedRolledResolvedChoreo",
            "generatedRolledResolvedTraceRole0",
            0,
            &rolled_resolved_role0,
        ),
        trace_proof_source(
            "generatedRejectingChoreo",
            "generatedRejectedTraceRole0",
            0,
            &rejected_role0,
        ),
        cyclic_roll_certificate::trace_source(&cyclic_roll_role2),
    ];
    let trace_count = trace_sources.len();
    let traces = trace_sources.join("\n");
    let mut projection_sources = vec![
        projection_certificate_source::<0>(&program, "generatedChoreo", "generatedProjectionRole0"),
        projection_certificate_source::<1>(&program, "generatedChoreo", "generatedProjectionRole1"),
        projection_certificate_source::<2>(&program, "generatedChoreo", "generatedProjectionRole2"),
        projection_certificate_source::<3>(&program, "generatedChoreo", "generatedProjectionRole3"),
        projection_certificate_source::<0>(
            &rolled,
            "generatedRolledChoreo",
            "generatedRolledProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &rolled,
            "generatedRolledChoreo",
            "generatedRolledProjectionRole1",
        ),
        projection_certificate_source::<0>(
            &nested_rolled,
            "generatedNestedRolledChoreo",
            "generatedNestedRolledProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &nested_rolled,
            "generatedNestedRolledChoreo",
            "generatedNestedRolledProjectionRole1",
        ),
        projection_certificate_source::<0>(
            &resolved,
            "generatedResolvedChoreo",
            "generatedResolvedProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &resolved,
            "generatedResolvedChoreo",
            "generatedResolvedProjectionRole1",
        ),
        projection_certificate_source::<0>(
            &nested_resolved,
            "generatedNestedResolvedChoreo",
            "generatedNestedResolvedProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &nested_resolved,
            "generatedNestedResolvedChoreo",
            "generatedNestedResolvedProjectionRole1",
        ),
        projection_certificate_source::<0>(
            &rolled_resolved,
            "generatedRolledResolvedChoreo",
            "generatedRolledResolvedProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &rolled_resolved,
            "generatedRolledResolvedChoreo",
            "generatedRolledResolvedProjectionRole1",
        ),
        projection_certificate_source::<0>(
            &rejecting,
            "generatedRejectingChoreo",
            "generatedRejectingProjectionRole0",
        ),
        projection_certificate_source::<1>(
            &rejecting,
            "generatedRejectingChoreo",
            "generatedRejectingProjectionRole1",
        ),
        projection_certificate_source::<254>(
            &full_role_domain,
            "generatedFullRoleDomainChoreo",
            "generatedFullRoleDomainProjectionRole254",
        ),
        projection_certificate_source::<255>(
            &full_role_domain,
            "generatedFullRoleDomainChoreo",
            "generatedFullRoleDomainProjectionRole255",
        ),
        projection_certificate_source::<3>(
            &lane_matching,
            "generatedLaneMatchingChoreo",
            "generatedLaneMatchingProjectionRole3",
        ),
    ];
    projection_sources.extend(cyclic_roll_certificate::projection_sources(&cyclic_roll));
    let projection_count = projection_sources.len();
    let projections = projection_sources.join("\n");
    let progress_sources = [
        progress_certificate_source("generatedChoreo", 0, "generatedProgressRole0"),
        progress_certificate_source(
            "generatedResolvedChoreo",
            0,
            "generatedResolvedProgressRole0",
        ),
        progress_certificate_source("generatedRolledChoreo", 0, "generatedRolledProgressRole0"),
        progress_certificate_source(
            "generatedNestedResolvedChoreo",
            0,
            "generatedNestedResolvedProgressRole0",
        ),
    ];
    let progress_count = progress_sources.len();
    let progress = progress_sources.join("\n");
    let mut projectability_sources = vec![
        projectability_certificate_source("generatedChoreo", 4, "generatedProjectability"),
        projectability_certificate_source(
            "generatedRolledChoreo",
            2,
            "generatedRolledProjectability",
        ),
        projectability_certificate_source(
            "generatedNestedRolledChoreo",
            2,
            "generatedNestedRolledProjectability",
        ),
        projectability_certificate_source(
            "generatedResolvedChoreo",
            2,
            "generatedResolvedProjectability",
        ),
        projectability_certificate_source(
            "generatedNestedResolvedChoreo",
            2,
            "generatedNestedResolvedProjectability",
        ),
        projectability_certificate_source(
            "generatedRolledResolvedChoreo",
            2,
            "generatedRolledResolvedProjectability",
        ),
        projectability_certificate_source(
            "generatedRejectingChoreo",
            2,
            "generatedRejectingProjectability",
        ),
    ];
    projectability_sources.push(cyclic_roll_certificate::projectability_source());
    let projectability_count = projectability_sources.len();
    let projectability = projectability_sources.join("\n");
    let mut verified_protocol_sources = vec![
        verified_protocol_certificate_source(
            "generatedChoreo",
            4,
            "generatedProjectability",
            &[
                "generatedProjectionRole0",
                "generatedProjectionRole1",
                "generatedProjectionRole2",
                "generatedProjectionRole3",
            ],
            "generatedVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedRolledChoreo",
            2,
            "generatedRolledProjectability",
            &[
                "generatedRolledProjectionRole0",
                "generatedRolledProjectionRole1",
            ],
            "generatedRolledVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedNestedRolledChoreo",
            2,
            "generatedNestedRolledProjectability",
            &[
                "generatedNestedRolledProjectionRole0",
                "generatedNestedRolledProjectionRole1",
            ],
            "generatedNestedRolledVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedResolvedChoreo",
            2,
            "generatedResolvedProjectability",
            &[
                "generatedResolvedProjectionRole0",
                "generatedResolvedProjectionRole1",
            ],
            "generatedResolvedVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedNestedResolvedChoreo",
            2,
            "generatedNestedResolvedProjectability",
            &[
                "generatedNestedResolvedProjectionRole0",
                "generatedNestedResolvedProjectionRole1",
            ],
            "generatedNestedResolvedVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedRolledResolvedChoreo",
            2,
            "generatedRolledResolvedProjectability",
            &[
                "generatedRolledResolvedProjectionRole0",
                "generatedRolledResolvedProjectionRole1",
            ],
            "generatedRolledResolvedVerifiedProtocol",
        ),
        verified_protocol_certificate_source(
            "generatedRejectingChoreo",
            2,
            "generatedRejectingProjectability",
            &[
                "generatedRejectingProjectionRole0",
                "generatedRejectingProjectionRole1",
            ],
            "generatedRejectingVerifiedProtocol",
        ),
    ];
    verified_protocol_sources.push(cyclic_roll_certificate::verified_protocol_source());
    let verified_protocol_count = verified_protocol_sources.len();
    let verified_protocols = verified_protocol_sources.join("\n");
    let production_kernel_artifact = production_kernel_artifact::source();
    let generated = format!(
        "import Hibana.MainTheorems\n\n\
         def generatedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRolledChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedNestedRolledChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedNestedResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRolledResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRejectingChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedCyclicRollChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedFullRoleDomainChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedLaneMatchingChoreo : Hibana.Choreo :=\n  {}\n\n\
         {}\n\
         {}\n\
         {}\n\
         {}\n\
         {}\n\
         {}\n\
         #eval IO.println \"hibana Lean generated proof passed traces={} frames={} projections={} exact-descriptors={} progress={} projectability={} distributed-progress={} verified-protocols={}\"\n",
        Steps::lean_source(),
        RolledSteps::lean_source(),
        NestedRolledSteps::lean_source(),
        ResolvedSteps::lean_source(),
        NestedResolvedSteps::lean_source(),
        RolledResolvedSteps::lean_source(),
        RejectSteps::lean_source(),
        cyclic_roll_certificate::lean_source(),
        FullRoleDomainSteps::lean_source(),
        MatchingSteps::lean_source(),
        traces,
        projections,
        progress,
        projectability,
        verified_protocols,
        production_kernel_artifact,
        trace_count,
        total_frames,
        projection_count,
        projection_count,
        progress_count,
        projectability_count,
        verified_protocol_count,
        verified_protocol_count,
    );
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&output_dir).expect("create generated Lean proof artifact directory");
    let output = output_dir.join("Generated.lean");
    fs::write(&output, generated).expect("write generated Lean proof artifact");
    println!("lean-proof-artifact path={}", output.display());
}
