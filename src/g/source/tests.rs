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
fn typed_owned_source_keeps_its_kind_and_full_role_in_the_compact_program_image() {
    use crate::eff::EventPayload;
    use crate::global::{
        compiled::{
            images::{ProgramImageBytes, ProgramImagePlan},
            lowering::CompiledProgramImage,
        },
        const_dsl::EffList,
    };

    type Grant = g::Msg<19, Endpoint<'static, 255>>;
    const SOURCE: super::ProgramSourceData<8> =
        super::ProgramSourceData::lower::<g::Send<0, 1, Grant>>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_program(SOURCE.eff_list());
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<9> =
        ProgramImageBytes::from_image(SOURCE.eff_list(), PLAN.columns());
    let program = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    assert_eq!(PLAN.blob_len(), 9);
    assert_eq!(SUMMARY.max_role(), 255);
    let grant = program.event_atom_at(0);
    assert_eq!((grant.from, grant.to, grant.label), (0, 1, 19));
    assert_eq!(grant.payload, EventPayload::Endpoint(255));
    assert_eq!(program.proof_byte_at(7), EventPayload::OWNED_FLAG);

    // A codec schema with the same numeric value produces different bytes
    // and a different outbound selector, even with identical header fields.
    const WIRE: EffList<1> = EffList::new().push(crate::eff::EffAtom {
        from: 0,
        to: 1,
        label: 19,
        payload: EventPayload::Wire(255),
        origin: crate::eff::EventOrigin::User,
        lane: 0,
    });
    const WIRE_PLAN: ProgramImagePlan = ProgramImagePlan::from_program(&WIRE);
    const WIRE_SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(&WIRE);
    static WIRE_BYTES: ProgramImageBytes<9> =
        ProgramImageBytes::from_image(&WIRE, WIRE_PLAN.columns());
    let wire = WIRE_BYTES.compiled_ref(&WIRE_SUMMARY, WIRE_PLAN.columns());
    assert_eq!(wire.event_atom_at(0).payload, EventPayload::Wire(255));
    assert_eq!(wire.proof_byte_at(7), 0);
    assert!(!program.same_image(&wire));
    assert_ne!(
        grant.payload.selector_bits(),
        wire.event_atom_at(0).payload.selector_bits()
    );
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
fn owned_kind_survives_parallel_rolled_loans_and_resolved_choices() {
    use crate::eff::EventPayload;
    use crate::global::compiled::{
        images::{ProgramImageBytes, ProgramImagePlan},
        lowering::CompiledProgramImage,
    };

    fn check<Steps: ProgramShape>(_: g::Program<Steps>) {
        let source = const { super::ProgramSourceData::<64>::lower::<Steps>() };
        let plan = ProgramImagePlan::from_program(source.eff_list());
        let summary = CompiledProgramImage::scan_const(source.eff_list());
        let bytes: &'static ProgramImageBytes<256> = &const {
            let source = super::ProgramSourceData::<64>::lower::<Steps>();
            let plan = ProgramImagePlan::from_program(source.eff_list());
            ProgramImageBytes::from_image(source.eff_list(), plan.columns())
        };
        let image = bytes.compiled_ref(&summary, plan.columns());

        assert_eq!(image.proof_atom_count(), 7);
        for index in 0..image.proof_atom_count() {
            assert_eq!(image.event_atom_at(index), source.eff_list().atom_at(index));
        }
        for (index, role) in [(0, DB), (3, DB), (4, AUTH), (6, AUTH)] {
            assert_eq!(
                image.event_atom_at(index).payload,
                EventPayload::Endpoint(role)
            );
        }
        assert!(matches!(
            image.event_atom_at(1).payload,
            EventPayload::Wire(_)
        ));
        assert!(matches!(
            image.event_atom_at(2).payload,
            EventPayload::Wire(_)
        ));
        assert!(matches!(
            image.event_atom_at(5).payload,
            EventPayload::Wire(_)
        ));
        assert_ne!(image.event_atom_at(1).lane, image.event_atom_at(5).lane);
        assert_eq!(
            usize::from(plan.columns().scope_markers().len),
            source.eff_list().scope_marker_count()
        );
        let mut resolvers = image.route_resolver_sites_for(7);
        let resolver = resolvers
            .next()
            .expect("the source's explicit decision site");
        assert_eq!(resolver.resolver_id(), 7);
        assert!(resolvers.next().is_none());
    }

    // This checks the actual typed source, source ownership constraints and
    // compact image. Public actor projection remains deliberately unavailable.
    check(g::par(
        g::seq(
            g::send::<P, A, GrantDb>(),
            g::seq(
                g::route(
                    g::send::<DB, SERVICE, Work>(),
                    g::send::<DB, SERVICE, ResultMessage>(),
                )
                .resolve::<7>(),
                g::send::<A, P, GrantDb>(),
            ),
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
