use super::{
    LeanChoreo, ProductionStep, ProofAction, ProofArm, ProofKey, projectability_certificate_source,
    projection_certificate_source, record_production_steps, trace_proof_source,
    verified_protocol_certificate_source,
};
use crate::g;
use std::{string::String, vec::Vec};

const RESOLVER: u16 = 906;

type Left = g::Seq<
    g::Send<2, 2, g::Msg<107, ()>>,
    g::Seq<
        g::Send<0, 2, g::Msg<101, ()>>,
        g::Seq<g::Send<2, 0, g::Msg<103, ()>>, g::Send<2, 1, g::Msg<104, ()>>>,
    >,
>;
type Right = g::Seq<
    g::Send<2, 2, g::Msg<108, ()>>,
    g::Seq<
        g::Send<1, 2, g::Msg<102, ()>>,
        g::Seq<g::Send<2, 0, g::Msg<105, ()>>, g::Send<2, 1, g::Msg<106, ()>>>,
    >,
>;
pub(super) type Steps = g::Roll<g::Resolve<g::Route<Left, Right>, RESOLVER>>;

pub(super) fn program() -> g::Program<Steps> {
    g::route(
        g::seq(
            g::send::<2, 2, g::Msg<107, ()>>(),
            g::seq(
                g::send::<0, 2, g::Msg<101, ()>>(),
                g::seq(
                    g::send::<2, 0, g::Msg<103, ()>>(),
                    g::send::<2, 1, g::Msg<104, ()>>(),
                ),
            ),
        ),
        g::seq(
            g::send::<2, 2, g::Msg<108, ()>>(),
            g::seq(
                g::send::<1, 2, g::Msg<102, ()>>(),
                g::seq(
                    g::send::<2, 0, g::Msg<105, ()>>(),
                    g::send::<2, 1, g::Msg<106, ()>>(),
                ),
            ),
        ),
    )
    .resolve::<RESOLVER>()
    .roll()
}

pub(super) fn trace(program: &g::Program<Steps>) -> Vec<(Vec<ProofKey>, ProofAction)> {
    record_production_steps::<2>(
        program,
        &[
            ProductionStep::Resolve {
                conflict: 0,
                resolver: RESOLVER,
                arm: ProofArm::Left,
            },
            ProductionStep::Commit(107),
            ProductionStep::Commit(101),
            ProductionStep::Commit(103),
            ProductionStep::Commit(104),
            ProductionStep::Resolve {
                conflict: 0,
                resolver: RESOLVER,
                arm: ProofArm::Right,
            },
            ProductionStep::Commit(108),
            ProductionStep::Commit(102),
            ProductionStep::Commit(105),
            ProductionStep::Commit(106),
        ],
    )
}

pub(super) fn trace_source(trace: &[(Vec<ProofKey>, ProofAction)]) -> String {
    trace_proof_source(
        "generatedCyclicRollChoreo",
        "generatedCyclicRollTraceRole2",
        2,
        trace,
    )
}

pub(super) fn projection_sources(program: &g::Program<Steps>) -> [String; 3] {
    [
        projection_certificate_source::<0>(
            program,
            "generatedCyclicRollChoreo",
            "generatedCyclicRollProjectionRole0",
        ),
        projection_certificate_source::<1>(
            program,
            "generatedCyclicRollChoreo",
            "generatedCyclicRollProjectionRole1",
        ),
        projection_certificate_source::<2>(
            program,
            "generatedCyclicRollChoreo",
            "generatedCyclicRollProjectionRole2",
        ),
    ]
}

pub(super) fn projectability_source() -> String {
    projectability_certificate_source(
        "generatedCyclicRollChoreo",
        3,
        "generatedCyclicRollProjectability",
    )
}

pub(super) fn verified_protocol_source() -> String {
    verified_protocol_certificate_source(
        "generatedCyclicRollChoreo",
        3,
        "generatedCyclicRollProjectability",
        &[
            "generatedCyclicRollProjectionRole0",
            "generatedCyclicRollProjectionRole1",
            "generatedCyclicRollProjectionRole2",
        ],
        "generatedCyclicRollVerifiedProtocol",
    )
}

pub(super) fn lean_source() -> String {
    <Steps as LeanChoreo>::lean_source()
}
