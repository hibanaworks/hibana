//! Isolate the new source obligation from selector and receive-lane checks.
//! Both resource holders are functions of the same canonical outer choice.
use super::*;

type RightDb = g::Msg<5, Endpoint<'static, DB>>;
type RightAuth = g::Msg<6, Endpoint<'static, AUTH>>;
type NoticeLeft = g::Msg<40, ()>;
type NoticeRight = g::Msg<41, ()>;
type Permit = g::Msg<38, ()>;
type Tick = g::Msg<39, ()>;
type Choose = g::Route<
    g::Seq<g::Send<P, A, GrantDb>, g::Seq<g::Send<P, A, GrantAuth>, g::Send<P, B, NoticeLeft>>>,
    g::Seq<g::Send<P, B, RightDb>, g::Seq<g::Send<P, B, RightAuth>, g::Send<P, A, NoticeRight>>>,
>;
type WorkBody<const GATE: u8> = g::Seq<
    g::Send<GATE, SERVICE, Permit>,
    g::Seq<g::Send<SERVICE, DB, Tick>, g::Send<DB, 4, Work>>,
>;
type Steps<const GATE: u8> =
    g::Seq<Choose, g::Seq<g::Roll<WorkBody<GATE>>, g::Send<AUTH, 5, GrantDb>>>;

#[test]
fn the_permit_and_resource_holder_use_the_same_selected_outer_choice() {
    let source = const { crate::g::source::ProgramSourceData::<32>::lower::<Steps<AUTH>>() };
    assert!(
        source
            .actors()
            .unwrap()
            .has_reentry_knowledge(&Steps::<AUTH>::SOURCE_NODE)
    );
}

#[test]
fn a_fixed_actor_cannot_authorize_both_resource_holders() {
    let source = const { crate::g::source::ProgramSourceData::<32>::lower::<Steps<A>>() };
    assert!(
        !source
            .actors()
            .unwrap()
            .has_reentry_knowledge(&Steps::<A>::SOURCE_NODE)
    );
}
