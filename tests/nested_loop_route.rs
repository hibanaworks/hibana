use hibana::g::{self, Msg, Role};
use hibana::substrate::cap::GenericCapToken;
use hibana::substrate::cap::advanced::{LoopBreakKind, LoopContinueKind};
use hibana::substrate::program::{RoleProgram, project};

const TEST_LOOP_CONTINUE_LOGICAL: u8 = 0xA1;
const TEST_LOOP_BREAK_LOGICAL: u8 = 0xA2;

#[test]
fn nested_loop_scope_balanced() {
    let tick = g::send::<Role<2>, Role<3>, Msg<1, ()>, 0>();
    let ack_branch = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >()
        .policy::<10>(),
        g::send::<
            Role<2>,
            Role<2>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >(),
    );
    let loss_branch = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >()
        .policy::<10>(),
        g::send::<
            Role<2>,
            Role<2>,
            Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            0,
        >(),
    );
    let ack_loss_route = g::route(ack_branch, loss_branch);
    let continue_arm = g::seq(
        g::send::<
            Role<2>,
            Role<2>,
            Msg<
                { TEST_LOOP_CONTINUE_LOGICAL },
                GenericCapToken<LoopContinueKind>,
                LoopContinueKind,
            >,
            0,
        >()
        .policy::<11>(),
        g::seq(tick, ack_loss_route),
    );
    let break_arm = g::send::<
        Role<2>,
        Role<2>,
        Msg<{ TEST_LOOP_BREAK_LOGICAL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
        0,
    >()
    .policy::<11>();
    let decision = g::route(continue_arm, break_arm);

    let role_program: RoleProgram<2> = project(&decision);
    drop(role_program);

    let handshake = g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
    let combined = g::par(handshake, decision);
    let transport_program: RoleProgram<2> = project(&combined);
    drop(transport_program);
}
