use super::{ProductionCursorTrace, selected_arm};
use crate::{
    g,
    global::{
        Message,
        const_dsl::{RouteResolver, ScopeId},
    },
};
use std::{
    format, fs,
    path::PathBuf,
    println,
    string::{String, ToString},
    vec::Vec,
};

trait LeanChoreo {
    fn lean_source() -> String;
}

impl<const FROM: u8, const TO: u8, M> LeanChoreo for g::Send<FROM, TO, M>
where
    M: Message,
{
    fn lean_source() -> String {
        format!("Hibana.Choreo.send {FROM} {TO} {}", M::LOGICAL_LABEL)
    }
}

impl<Left, Right> LeanChoreo for g::Seq<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.seq ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right> LeanChoreo for g::Par<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.par ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right> LeanChoreo for g::Route<Left, Right>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.route .intrinsic ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Left, Right, const RESOLVER_ID: u16> LeanChoreo
    for g::Resolve<g::Route<Left, Right>, RESOLVER_ID>
where
    Left: LeanChoreo,
    Right: LeanChoreo,
{
    fn lean_source() -> String {
        format!(
            "Hibana.Choreo.route (.dynamic {RESOLVER_ID}) ({}) ({})",
            Left::lean_source(),
            Right::lean_source()
        )
    }
}

impl<Inner> LeanChoreo for g::Roll<Inner>
where
    Inner: LeanChoreo,
{
    fn lean_source() -> String {
        format!("Hibana.Choreo.roll ({})", Inner::lean_source())
    }
}

fn lean_nat_list(values: &[u8]) -> String {
    let body = values
        .iter()
        .map(u8::to_string)
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

impl ProductionStep {
    const fn action(self) -> ProofAction {
        match self {
            Self::Commit(label) => ProofAction::Commit(label),
            Self::Resolve {
                conflict,
                resolver,
                arm,
            } => ProofAction::Resolve {
                conflict,
                resolver,
                arm,
            },
            Self::Reject { conflict, resolver } => ProofAction::Reject { conflict, resolver },
        }
    }
}

impl ProofAction {
    fn lean_source(self) -> String {
        match self {
            Self::Commit(label) => format!(".commit {label}"),
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

fn frame_source(enabled: &[u8], action: ProofAction) -> String {
    format!(
        "  {{ enabled := {}, action := {} }}",
        lean_nat_list(enabled),
        action.lean_source()
    )
}

fn record_trace<const ROLE: u8>(
    program: &impl crate::global::program::Projectable,
    commits: &[u8],
) -> Vec<(Vec<u8>, ProofAction)> {
    let mut production = ProductionCursorTrace::new::<ROLE>(program);
    let mut frames = Vec::new();
    for &label in commits {
        let mut enabled = production.enabled_labels();
        enabled.sort_unstable();
        assert!(
            enabled.contains(&label),
            "Lean proof fixture attempted disabled label {label}; enabled={enabled:?}"
        );
        frames.push((enabled, ProofAction::Commit(label)));
        production.commit_label(label);
    }
    let mut enabled = production.enabled_labels();
    enabled.sort_unstable();
    frames.push((enabled, ProofAction::Stop));
    frames
}

impl ProductionCursorTrace {
    fn proof_dynamic_scope(&self, conflict: u16, expected_resolver: u16) -> ScopeId {
        let scope = self
            .event_program
            .route_scope_rows_by_slot(conflict as usize)
            .expect("Lean proof conflict must name a production route slot")
            .scope();
        let (resolver, _) = self
            .cursor()
            .route_scope_controller_resolver(scope)
            .expect("Lean proof resolver site must exist in the production descriptor");
        match resolver {
            RouteResolver::Intrinsic => {
                panic!("Lean proof resolver site unexpectedly used intrinsic authority")
            }
            RouteResolver::Dynamic {
                resolver_id,
                scope: resolver_scope,
            } => {
                assert_eq!(resolver_scope, scope, "resolver scope metadata diverged");
                assert_eq!(
                    resolver_id, expected_resolver,
                    "resolver id metadata diverged"
                );
                scope
            }
        }
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
            !self.enabled_labels().is_empty(),
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
) -> Vec<(Vec<u8>, ProofAction)> {
    let mut production = ProductionCursorTrace::new::<ROLE>(program);
    let mut frames = Vec::new();
    for (index, &step) in steps.iter().enumerate() {
        let mut enabled = production.enabled_labels();
        enabled.sort_unstable();
        frames.push((enabled.clone(), step.action()));
        match step {
            ProductionStep::Commit(label) => {
                assert!(
                    enabled.contains(&label),
                    "Lean resolver fixture attempted disabled label {label}; enabled={enabled:?}"
                );
                production.commit_label(label);
            }
            ProductionStep::Resolve {
                conflict,
                resolver,
                arm,
            } => production.apply_proof_resolver_selection(conflict, resolver, arm),
            ProductionStep::Reject { conflict, resolver } => {
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
    let mut enabled = production.enabled_labels();
    enabled.sort_unstable();
    frames.push((enabled, ProofAction::Stop));
    frames
}

fn trace_proof_source(
    choreo: &str,
    name: &str,
    role: u8,
    frames: &[(Vec<u8>, ProofAction)],
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

    type A = g::Send<0, 1, g::Msg<11, ()>>;
    type B = g::Send<0, 2, g::Msg<12, ()>>;
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
    type ResolvedLeft = g::Send<0, 1, g::Msg<51, ()>>;
    type ResolvedRight = g::Send<0, 1, g::Msg<52, ()>>;
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

    let program = g::seq(
        g::par(
            g::send::<0, 1, g::Msg<11, ()>>(),
            g::send::<0, 2, g::Msg<12, ()>>(),
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
        g::send::<0, 1, g::Msg<51, ()>>(),
        g::send::<0, 1, g::Msg<52, ()>>(),
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
            ProductionStep::Commit(52),
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
        + rejected_role0.len();
    let proof_sources = [
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
    ];
    let artifact_count = proof_sources.len();
    let proofs = proof_sources.join("\n");
    let generated = format!(
        "import Hibana.MainTheorems\n\n\
         def generatedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRolledChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedNestedRolledChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedNestedResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRolledResolvedChoreo : Hibana.Choreo :=\n  {}\n\n\
         def generatedRejectingChoreo : Hibana.Choreo :=\n  {}\n\n\
         {}\n\
         #eval IO.println \"hibana Lean trace proof passed artifacts={} frames={}\"\n",
        Steps::lean_source(),
        RolledSteps::lean_source(),
        NestedRolledSteps::lean_source(),
        ResolvedSteps::lean_source(),
        NestedResolvedSteps::lean_source(),
        RolledResolvedSteps::lean_source(),
        RejectSteps::lean_source(),
        proofs,
        artifact_count,
        total_frames,
    );
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&output_dir).expect("create generated Lean proof artifact directory");
    let output = output_dir.join("Generated.lean");
    fs::write(&output, generated).expect("write generated Lean proof artifact");
    println!("lean-proof-artifact path={}", output.display());
}
