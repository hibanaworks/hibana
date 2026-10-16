use super::{ProgramShape, ownership, tree};
use crate::{Endpoint, g};

const P: u8 = 0;
const A: u8 = 1;
const B: u8 = 2;
const SERVICE: u8 = 3;
const DB: u8 = 10;
const AUTH: u8 = 11;
type GrantDb = g::Msg<1, Endpoint<'static, DB>>;
type GrantAuth = g::Msg<2, Endpoint<'static, AUTH>>;
type Work = g::Msg<3, u32>;
type ResultMessage = g::Msg<4, u32>;

fn source<Steps: ProgramShape>(_: g::Program<Steps>) -> &'static tree::Source<'static> {
    &Steps::SOURCE_NODE
}

fn validate(source: &tree::Source<'_>) -> Result<(), ownership::ErrorKind> {
    let mut scratch = ownership::Scratch::<256, 64>::new();
    ownership::validate(source, &mut scratch).map_err(|error| error.kind)
}

#[test]
fn typed_owned_message_has_no_wire_codec_or_pointer_representation() {
    let source = source(g::send::<P, A, GrantDb>());
    let tree::Source::Send(message) = source else {
        panic!("one source message");
    };
    assert_eq!(
        message.payload,
        tree::Payload::Endpoint(tree::EndpointKey {
            component: 0,
            role: DB
        })
    );
    assert_eq!(message.from.role, P);
    assert_eq!(message.to.role, A);
    assert_eq!(validate(source), Ok(()));
    static_assertions::assert_not_impl_any!(Endpoint<'static, DB>:
        crate::transport::wire::WireEncode,
        crate::transport::wire::WirePayload,
        Clone,
        Copy,
        Send,
        Sync);
}

#[test]
fn typed_global_checks_handoff_work_and_return_as_one_source() {
    let source = source(g::seq(
        g::send::<P, A, GrantDb>(),
        g::seq(
            g::send::<DB, SERVICE, Work>(),
            g::seq(
                g::send::<SERVICE, DB, ResultMessage>(),
                g::send::<A, P, GrantDb>(),
            ),
        ),
    ));
    assert!(source.has_delegation());
    assert_eq!(source.event_count(), 4);
    assert_eq!(validate(source), Ok(()));
}

#[test]
fn typed_parallel_handoff_cannot_race_resource_work() {
    let source = source(g::par(
        g::send::<P, A, GrantDb>(),
        g::send::<DB, SERVICE, Work>(),
    ));
    assert_eq!(
        validate(source),
        Err(ownership::ErrorKind::ParallelOwnershipConflict)
    );
}

#[test]
fn typed_global_rejects_an_unrelated_second_issuer() {
    let source = source(g::seq(
        g::send::<P, A, GrantDb>(),
        g::send::<B, A, GrantDb>(),
    ));
    assert_eq!(
        validate(source),
        Err(ownership::ErrorKind::ConflictingInitialOwners)
    );
}

#[test]
fn typed_roll_cannot_reissue_a_moved_endpoint() {
    let source = source(g::send::<P, A, GrantDb>().roll());
    assert_eq!(
        validate(source),
        Err(ownership::ErrorKind::RollChangesOwner)
    );
}

#[test]
fn typed_parallel_rolled_loans_keep_both_independent_resources() {
    let source = source(g::par(
        g::seq(
            g::send::<P, A, GrantDb>(),
            g::seq(g::send::<DB, SERVICE, Work>(), g::send::<A, P, GrantDb>()),
        )
        .roll(),
        g::seq(
            g::send::<P, B, GrantAuth>(),
            g::seq(
                g::send::<AUTH, SERVICE, Work>(),
                g::send::<B, P, GrantAuth>(),
            ),
        )
        .roll(),
    ));
    assert_eq!(validate(source), Ok(()));
}

#[test]
fn typed_resolved_choice_keeps_conditional_ownership_in_its_continuation() {
    let source = source(g::seq(
        g::route(g::send::<P, A, GrantDb>(), g::send::<P, B, GrantDb>()).resolve::<65535>(),
        g::seq(
            g::send::<DB, SERVICE, Work>(),
            g::send::<SERVICE, DB, ResultMessage>(),
        )
        .roll(),
    ));
    let tree::Source::Seq(tree::Source::Route { resolver, .. }, _) = source else {
        panic!("resolved route followed by a rolled continuation");
    };
    assert_eq!(*resolver, Some(65535));
    assert_eq!(validate(source), Ok(()));
}
