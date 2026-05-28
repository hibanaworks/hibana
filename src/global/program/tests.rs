use super::Program;
use crate::g;
use crate::global::steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
use crate::integration::cap::control::{LoopBreakKind, LoopContinueKind};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;

fn loop_continue_only() -> Program<
    SeqSteps<
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
            >,
            StepNil,
        >,
        StepNil,
    >,
> {
    g::seq(
        g::send::<
            g::Role<0>,
            g::Role<0>,
            g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
            0,
        >(),
        Program::<StepNil>::empty(),
    )
}

fn loop_break_only() -> Program<
    StepCons<
        SendStep<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>>,
        StepNil,
    >,
> {
    g::send::<g::Role<0>, g::Role<0>, g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>, 0>()
}

fn loop_decision() -> Program<
    RouteSteps<
        SeqSteps<
            StepCons<
                SendStep<
                    g::Role<0>,
                    g::Role<0>,
                    g::Msg<{ TEST_LOOP_CONTINUE_LOGICAL }, (), LoopContinueKind>,
                >,
                StepNil,
            >,
            StepNil,
        >,
        StepCons<
            SendStep<
                g::Role<0>,
                g::Role<0>,
                g::Msg<{ TEST_LOOP_BREAK_LOGICAL }, (), LoopBreakKind>,
            >,
            StepNil,
        >,
    >,
> {
    g::route(loop_continue_only(), loop_break_only())
}

#[test]
fn seq_with_empty_suffix_preserves_loop_tail_hint() {
    let composed = g::seq(loop_continue_only(), Program::<StepNil>::empty());
    assert!(
        composed.tail_is_loop_control(),
        "empty seq suffix must preserve loop-control tail hints"
    );
}

#[test]
fn empty_seq_suffix_does_not_change_pending_loop_scope_attachment() {
    let direct = g::seq(loop_continue_only(), loop_decision());
    let nested = g::seq(
        g::seq(loop_continue_only(), Program::<StepNil>::empty()),
        loop_decision(),
    );
    assert!(
        direct
            .program_image()
            .equivalent_summary(nested.program_image()),
        "empty seq suffix must not change the resident compiled program image"
    );
}
