use super::{ProgramShape, ownership, tree};
use crate::{Endpoint, g};

mod receive_causality;
mod reentry_knowledge;

#[test]
fn first_transfer_matches_linear_source_enumeration_for_every_small_tree() {
    use tree::{EndpointKey, Message, Payload, Source};
    fn pair<'a>(a: &'a Source<'a>, b: &'a Source<'a>, shape: u8) -> Source<'a> {
        match shape {
            0 => Source::seq(a, b),
            1 => Source::par(a, b),
            _ => Source::route(a, b, (shape == 3).then_some(7)),
        }
    }
    let keys = [
        EndpointKey {
            component: 0,
            role: 0,
        },
        EndpointKey {
            component: 1,
            role: 0,
        },
        EndpointKey {
            component: u16::MAX,
            role: u8::MAX,
        },
    ];
    let mut checked = 0;
    for word in 0usize..256 {
        let leaves: [_; 4] = core::array::from_fn(|index| {
            Source::Send(Message {
                from: keys[0],
                to: keys[1],
                label: index as u8,
                payload: match (word >> (2 * index)) & 3 {
                    3 => Payload::Wire { schema: 0 },
                    value => Payload::Endpoint(keys[value]),
                },
            })
        });
        for left_shape in 0..4 {
            for right_shape in 0..4 {
                let left = pair(&leaves[0], &leaves[1], left_shape);
                let right = pair(&leaves[2], &leaves[3], right_shape);
                let rolled = Source::roll(&right);
                for root_shape in 0..4 {
                    let root = pair(&left, &rolled, root_shape);
                    assert_eq!(root.event_count(), leaves.len());
                    assert_eq!(
                        root.has_delegation(),
                        leaves.iter().any(|leaf| {
                            matches!(
                                leaf,
                                Source::Send(Message {
                                    payload: Payload::Endpoint(_),
                                    ..
                                })
                            )
                        })
                    );
                    for event in 0..leaves.len() {
                        for lower in 0..=leaves.len() {
                            let mut found = [false; 4];
                            let mut next = lower;
                            while let Some((start, end)) = root.predecessor_range(event, next) {
                                assert!(next <= start && start < end && end <= event);
                                for member in &mut found[start..end] {
                                    assert!(!*member);
                                    *member = true;
                                }
                                next = end;
                            }
                            for (prior, &present) in found.iter().enumerate() {
                                assert_eq!(present, prior >= lower && root.before(prior, event));
                            }
                        }
                    }
                    for key in keys {
                        let expected = (0..root.event_count()).find(|&event| {
                            matches!(root.message_at(event).payload, Payload::Endpoint(payload) if payload.same(key))
                        });
                        assert_eq!(root.first_transfer(key), expected);
                        assert_eq!(root.transfers(key), expected.is_some());
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 49_152);
}

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
fn subtree_event_counts_match_the_typed_shape_and_all_message_leaves() {
    fn count_leaves(node: &tree::Source<'_>) -> usize {
        let leaves = match node {
            tree::Source::Send(_) => 1,
            tree::Source::Seq(left, right, _)
            | tree::Source::Par(left, right, _)
            | tree::Source::Route { left, right, .. } => count_leaves(left) + count_leaves(right),
            tree::Source::Roll(body, _) => count_leaves(body),
        };
        assert_eq!(node.event_count(), leaves);
        leaves
    }
    type Step = g::Send<0, 1, Work>;
    type Pair = g::Seq<Step, Step>;
    type Steps = g::Seq<
        g::Par<g::Roll<g::Route<Pair, Step>>, g::Seq<Pair, Step>>,
        g::Resolve<g::Route<Step, Pair>, 11>,
    >;
    let leaves = count_leaves(&Steps::SOURCE_NODE);
    assert_eq!(leaves, Steps::EVENT_COUNT);
    assert_eq!(leaves, 9);
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
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<19> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("exact owned image storage"),
        };
    let program = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    assert_eq!(PLAN.blob_len(), 19);
    assert_eq!(program.actor_binding_at(0), Some([0, 1]));
    assert_eq!(program.initial_actor_for(255), Some(0));
    assert_eq!(program.initial_actor_for(0), None);
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
fn typed_owned_operations_and_control_communications_compile_to_the_same_actor() {
    let source = source(g::seq(
        g::send::<P, A, GrantDb>(),
        g::seq(
            g::send::<DB, SERVICE, Work>(),
            g::seq(
                g::send::<SERVICE, DB, ResultMessage>(),
                g::seq(g::send::<SERVICE, A, Work>(), g::send::<A, P, GrantDb>()),
            ),
        ),
    ));
    let mut scratch = ownership::Scratch::<256, 8>::new();
    let mut image = ownership::ActorImage::<8, 0>::new();
    ownership::compile_actor_image_into(source, &mut scratch, &mut image).unwrap();
    assert_eq!(image.event_count(), 5);
    assert_eq!(image.actors_at(0, &[]), Some((P, A)));
    assert_eq!(image.actors_at(1, &[]), Some((A, SERVICE)));
    assert_eq!(image.actors_at(2, &[]), Some((SERVICE, A)));
    assert_eq!(image.actors_at(3, &[]), Some((SERVICE, A)));
    assert_eq!(image.actors_at(4, &[]), Some((A, P)));
    assert_eq!(image.decision_count(), 0);
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
        let plan = ProgramImagePlan::from_source(&source);
        let summary = CompiledProgramImage::scan_const(source.eff_list());
        let bytes: &'static ProgramImageBytes<256> = &const {
            let source = super::ProgramSourceData::<64>::lower::<Steps>();
            let plan = ProgramImagePlan::from_source(&source);
            match ProgramImageBytes::from_source_if_fits(&source, plan.columns()) {
                Some(bytes) => bytes,
                None => panic!("owned source image storage"),
            }
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
            image.source_scope_marker_count(),
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
fn compact_actor_decisions_reuse_canonical_scopes_and_keep_logical_roles() {
    use crate::global::{
        compiled::{
            images::{ProgramImageBytes, ProgramImagePlan},
            lowering::CompiledProgramImage,
        },
        const_dsl::{ScopeId, ScopeKind},
    };
    fn check<Steps: ProgramShape>(_: g::Program<Steps>) {
        let source = const { super::ProgramSourceData::<64>::lower::<Steps>() };
        let plan = ProgramImagePlan::from_source(&source);
        let summary = CompiledProgramImage::scan_const(source.eff_list());
        let bytes: &'static ProgramImageBytes<256> = &const {
            let source = super::ProgramSourceData::<64>::lower::<Steps>();
            let plan = ProgramImagePlan::from_source(&source);
            match ProgramImageBytes::from_source_if_fits(&source, plan.columns()) {
                Some(bytes) => bytes,
                None => panic!("owned conditional image storage"),
            }
        };
        let image = bytes.compiled_ref(&summary, plan.columns());
        assert_eq!(image.actor_binding_at(2), Some([256, SERVICE as u16]));
        assert_eq!(image.actor_binding_at(3), Some([SERVICE as u16, 256]));
        assert_eq!(
            image.actor_decision_at(0),
            Some((ScopeId::new(ScopeKind::Route, 1), [A as u16, B as u16]))
        );
        assert_eq!(image.actor_decision_at(1), None);
        assert_eq!(image.initial_actor_for(DB), Some(P));
        assert_eq!(image.initial_actor_for(AUTH), Some(P));
        assert_eq!(image.initial_actor_for(A), None);
        assert_eq!(image.event_atom_at(2).from, DB);
        assert_eq!(image.event_atom_at(3).to, DB);
    }
    check(g::par(
        g::seq(
            g::route(g::send::<P, A, GrantDb>(), g::send::<P, B, GrantDb>()).resolve::<701>(),
            g::seq(
                g::send::<DB, SERVICE, Work>(),
                g::send::<SERVICE, DB, ResultMessage>(),
            )
            .roll(),
        ),
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
fn distinct_logical_endpoints_at_one_actor_keep_their_payload_communication() {
    use crate::global::compiled::{
        images::{ProgramImageBytes, ProgramImagePlan},
        lowering::CompiledProgramImage,
    };
    type Steps =
        g::Seq<g::Send<P, A, GrantDb>, g::Seq<g::Send<DB, A, Work>, g::Send<A, P, GrantDb>>>;
    const SOURCE: super::ProgramSourceData<8> = super::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<64> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("same-actor logical communication storage"),
        };
    let image = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    assert_eq!(image.actor_binding_at(1), Some([A as u16, A as u16]));
    let work = image.event_atom_at(1);
    assert_eq!((work.from, work.to), (DB, A));
    assert_eq!(
        work.payload.wire_schema(),
        crate::global::payload_kind::<Work>().wire_schema()
    );
    assert_ne!(work.payload.wire_schema(), 0);
    assert_eq!(image.actor_binding_at(2), Some([A as u16, P as u16]));
    assert_eq!(image.initial_actor_for(DB), Some(P));
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
    let tree::Source::Seq(tree::Source::Route { resolver, .. }, _, _) = source else {
        panic!("resolved route followed by a rolled continuation");
    };
    assert_eq!(*resolver, Some(65535));
    assert_eq!(validate(source), Ok(()));
}
