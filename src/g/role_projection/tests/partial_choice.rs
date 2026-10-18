//! A nested global determines the holder without a mutable ownership table.
use super::*;

type FirstNotice = g::Msg<5, ()>;
type SecondNotice = g::Msg<6, ()>;
type ThirdNotice = g::Msg<7, ()>;

pub(super) type Steps = g::Seq<
    g::Route<
        g::Route<
            g::Seq<
                g::Send<0, 1, Loan>,
                g::Seq<g::Send<0, 2, FirstNotice>, g::Send<0, 3, FirstNotice>>,
            >,
            g::Seq<
                g::Send<0, 1, SecondNotice>,
                g::Seq<g::Send<0, 2, Loan>, g::Send<0, 3, SecondNotice>>,
            >,
        >,
        g::Seq<g::Send<0, 1, ThirdNotice>, g::Seq<g::Send<0, 2, ThirdNotice>, g::Send<0, 3, Loan>>>,
    >,
    g::Seq<g::Send<10, 4, Read>, g::Send<4, 10, Value>>,
>;

// Source-level outcomes: the outer right arm lends DB to actor 3. Only the
// outer left arm needs the inner choice, which lends DB to actor 1 or actor 2.
// These include incomplete evidence; they are not another execution cursor.
pub(super) const CASES: [(Option<u8>, Option<u8>, Option<u8>); 9] = [
    (None, None, None),
    (None, Some(0), None),
    (None, Some(1), None),
    (Some(0), None, None),
    (Some(0), Some(0), Some(1)),
    (Some(0), Some(1), Some(2)),
    (Some(1), None, Some(3)),
    (Some(1), Some(0), Some(3)),
    (Some(1), Some(1), Some(3)),
];

#[test]
fn partial_choice_evidence_never_guesses_or_changes_a_known_holder() {
    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let program = g::ProgramProjection::<Steps, 32>::PROGRAM_REF;
    let [reference, service] = program
        .actor_binding_at(program.columns.atom_count() - 2)
        .unwrap();
    assert_eq!(service, 4);
    let (outer_scope, [inner_reference, right_actor]) = program
        .actor_decision_at(usize::from(reference) - 256)
        .unwrap();
    let (inner_scope, inner_actors) = program
        .actor_decision_at(usize::from(inner_reference) - 256)
        .unwrap();
    assert_eq!(right_actor, 3);
    assert_eq!(inner_actors, [1, 2]);
    assert_ne!(outer_scope, inner_scope);

    for (outer, inner, expected) in CASES {
        let observed = program.selected_actor(reference, &mut |scope| {
            if scope == outer_scope {
                outer
            } else {
                assert_eq!(scope, inner_scope);
                inner
            }
        });
        assert_eq!(observed, expected, "outer={outer:?} inner={inner:?}");

        let Some(actor) = observed else { continue };
        for (extended_outer, extended_inner, _) in CASES {
            if outer.is_some() && outer != extended_outer
                || inner.is_some() && inner != extended_inner
            {
                continue;
            }
            assert_eq!(
                program.selected_actor(reference, &mut |scope| {
                    if scope == outer_scope {
                        extended_outer
                    } else {
                        assert_eq!(scope, inner_scope);
                        extended_inner
                    }
                }),
                Some(actor),
                "known=({outer:?}, {inner:?}) extended=({extended_outer:?}, {extended_inner:?})",
            );
        }
    }
}

#[test]
fn conditional_resource_admission_requires_a_known_local_holder() {
    use crate::global::typestate::EventAdmissionError;

    let images = [
        (1, &RoleProjection::<1, Steps, 32>::IMAGE_REF),
        (2, &RoleProjection::<2, Steps, 32>::IMAGE_REF),
        (3, &RoleProjection::<3, Steps, 32>::IMAGE_REF),
    ];
    let program = g::ProgramProjection::<Steps, 32>::PROGRAM_REF;
    let [reference, _] = program
        .actor_binding_at(program.columns.atom_count() - 2)
        .unwrap();
    let (outer_scope, [inner_reference, _]) = program
        .actor_decision_at(usize::from(reference) - 256)
        .unwrap();
    let (inner_scope, _) = program
        .actor_decision_at(usize::from(inner_reference) - 256)
        .unwrap();

    for (actor, image) in images {
        with_cursor(image, |cursor| {
            let read = (0..image.local_step_count())
                .find(|&idx| {
                    cursor.logical_role_at(idx) == Some(10)
                        && image.local_step_node(idx).unwrap().action().is_send()
                })
                .unwrap();
            for (outer, inner, expected) in CASES {
                let result = cursor.event_enabled(read, event(image, read), &mut |scope, _| {
                    if scope == outer_scope {
                        outer
                    } else {
                        assert_eq!(scope, inner_scope);
                        inner
                    }
                });
                match expected {
                    None => assert!(matches!(
                        result,
                        Err(EventAdmissionError::PredecessorPending)
                    )),
                    Some(holder) if holder != actor => {
                        assert!(matches!(result, Err(EventAdmissionError::Invariant)));
                    }
                    Some(_) => {
                        // Correct ownership alone does not skip the preceding
                        // loan receive in this actor's shared progress.
                        assert!(matches!(
                            result,
                            Err(EventAdmissionError::PredecessorPending
                                | EventAdmissionError::OtherViewPending)
                        ));
                    }
                }
            }
        });
    }
}

#[test]
fn a_service_uses_the_delegated_peer_without_knowing_its_physical_holder() {
    use super::distributed_source::{FrameBoundary, OperationObservation};
    use crate::{
        endpoint::proof_snapshot::actor_snapshot,
        runtime::{SessionKitStorage, ids::SessionId},
        test_support::transport::TestTransport,
    };

    let () = g::ProgramProjection::<Steps, 32>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let first_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let second_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 32>::IMAGE_REF,
    );
    let third_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    #[cfg(not(miri))]
    let mut proof = {
        use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
        use std::fmt::Write;
        let mut text = std::format!(
            "import Hibana.SourceOwnedReceive\n\nopen Hibana Hibana.EndpointDelegation\n\n\
             set_option maxRecDepth 10000\n\ndef peerWitnessChoreo : Choreo := {}\n\n",
            Steps::lean_source(),
        );
        for (actor, image) in [
            (1, &RoleProjection::<1, Steps, 32>::IMAGE_REF),
            (2, &RoleProjection::<2, Steps, 32>::IMAGE_REF),
            (3, &RoleProjection::<3, Steps, 32>::IMAGE_REF),
            (4, &RoleProjection::<4, Steps, 32>::IMAGE_REF),
        ] {
            writeln!(
                text,
                "def peerWitnessImage{actor} : RustDescriptorImage := {}\n",
                descriptor_source(RoleDescriptorRef::from_resident(image))
            )
            .unwrap();
        }
        text
    };

    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            // This is test input selecting the intrinsic route, not a record
            // of ownership or protocol progress. Each localside follows offer.
            for holder in [1u8, 2, 3] {
                let network = TestTransport::new();
                let mut owner_slab = [0; 32 * 1024];
                let mut first_slab = [0; 32 * 1024];
                let mut second_slab = [0; 32 * 1024];
                let mut third_slab = [0; 32 * 1024];
                let mut service_slab = [0; 32 * 1024];
                let mut owner_storage = SessionKitStorage::uninit();
                let mut first_storage = SessionKitStorage::uninit();
                let mut second_storage = SessionKitStorage::uninit();
                let mut third_storage = SessionKitStorage::uninit();
                let mut service_storage = SessionKitStorage::uninit();
                let owner_rv = owner_storage.init().rendezvous(&mut owner_slab, network.clone()).unwrap();
                let first_rv = first_storage.init().rendezvous(&mut first_slab, network.clone()).unwrap();
                let second_rv = second_storage.init().rendezvous(&mut second_slab, network.clone()).unwrap();
                let third_rv = third_storage.init().rendezvous(&mut third_slab, network.clone()).unwrap();
                let service_rv = service_storage.init().rendezvous(&mut service_slab, network.clone()).unwrap();
                let sid = SessionId::new(9070 + u32::from(holder));
                let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
                let db = owner_rv.enter(sid, &db_program).unwrap();
                let mut first = first_rv.enter(sid, &first_program).unwrap();
                let mut second = second_rv.enter(sid, &second_program).unwrap();
                let mut third = third_rv.enter(sid, &third_program).unwrap();
                let mut service = service_rv.enter(sid, &service_program).unwrap();
                let ((), from_first, from_second, from_third, received) =
                    tokio::time::timeout(core::time::Duration::from_secs(30), async {
                        tokio::join!(
                            async {
                                match holder {
                                    1 => {
                                        owner.send::<Loan>(db).await.unwrap();
                                        owner.send::<FirstNotice>(&()).await.unwrap();
                                        owner.send::<FirstNotice>(&()).await.unwrap();
                                    }
                                    2 => {
                                        owner.send::<SecondNotice>(&()).await.unwrap();
                                        owner.send::<Loan>(db).await.unwrap();
                                        owner.send::<SecondNotice>(&()).await.unwrap();
                                    }
                                    3 => {
                                        owner.send::<ThirdNotice>(&()).await.unwrap();
                                        owner.send::<ThirdNotice>(&()).await.unwrap();
                                        owner.send::<Loan>(db).await.unwrap();
                                    }
                                    _ => unreachable!(),
                                }
                            },
                            async {
                                let branch = first.offer().await.unwrap();
                                match branch.label() {
                                    1 => {
                                        let mut db = branch.recv::<Loan>().await.unwrap();
                                        let before = unsafe { actor_snapshot(&db) };
                                        db.send::<Read>(&7).await.unwrap();
                                        let observed = OperationObservation {
                                            view: 10, label: 3, before,
                                            after: unsafe { actor_snapshot(&db) },
                                            frame: FrameBoundary::Sent(network.accepted_frame(4, 0)),
                                        };
                                        assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                                        Some(observed)
                                    }
                                    6 => { branch.recv::<SecondNotice>().await.unwrap(); None }
                                    7 => { branch.recv::<ThirdNotice>().await.unwrap(); None }
                                    label => panic!("unexpected first worker branch {label}"),
                                }
                            },
                            async {
                                let branch = second.offer().await.unwrap();
                                match branch.label() {
                                    1 => {
                                        let mut db = branch.recv::<Loan>().await.unwrap();
                                        let before = unsafe { actor_snapshot(&db) };
                                        db.send::<Read>(&7).await.unwrap();
                                        let observed = OperationObservation {
                                            view: 10, label: 3, before,
                                            after: unsafe { actor_snapshot(&db) },
                                            frame: FrameBoundary::Sent(network.accepted_frame(4, 0)),
                                        };
                                        assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                                        Some(observed)
                                    }
                                    5 => { branch.recv::<FirstNotice>().await.unwrap(); None }
                                    7 => { branch.recv::<ThirdNotice>().await.unwrap(); None }
                                    label => panic!("unexpected second worker branch {label}"),
                                }
                            },
                            async {
                                let branch = third.offer().await.unwrap();
                                match branch.label() {
                                    1 => {
                                        let mut db = branch.recv::<Loan>().await.unwrap();
                                        let before = unsafe { actor_snapshot(&db) };
                                        db.send::<Read>(&7).await.unwrap();
                                        let observed = OperationObservation {
                                            view: 10, label: 3, before,
                                            after: unsafe { actor_snapshot(&db) },
                                            frame: FrameBoundary::Sent(network.accepted_frame(4, 0)),
                                        };
                                        assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                                        Some(observed)
                                    }
                                    5 => { branch.recv::<FirstNotice>().await.unwrap(); None }
                                    6 => { branch.recv::<SecondNotice>().await.unwrap(); None }
                                    label => panic!("unexpected third worker branch {label}"),
                                }
                            },
                            async {
                                let before = unsafe { actor_snapshot(&service) };
                                let number = service.recv::<Read>().await.unwrap();
                                let observed = OperationObservation {
                                    view: 4, label: 3, before,
                                    after: unsafe { actor_snapshot(&service) },
                                    frame: FrameBoundary::Received(network.received_frame(4, sid, 0)),
                                };
                                assert_eq!(number, 7);
                                service.send::<Value>(&(number + 1)).await.unwrap();
                                observed
                            },
                        )
                    }).await.unwrap();
                // Passive records are inspected only after all five localsides
                // finish. No endpoint operation consults them.
                let sources = [from_first, from_second, from_third];
                assert_eq!(sources.iter().filter(|source| source.is_some()).count(), 1);
                let sent = sources.into_iter().flatten().next().unwrap();
                assert_eq!(sent.before.actor, holder);
                assert_eq!(received.before.actor, 4);
                assert_eq!(received.before.choices.len(), 2);
                assert_eq!(received.after.choices, received.before.choices);
                assert!(received.before.choices.iter().all(|(_, arm)| arm.is_none()));
                assert_eq!(sent.frame().header, received.frame().header);
                assert_eq!(sent.frame().payload(), received.frame().payload());
                #[cfg(not(miri))]
                {
                    use std::fmt::Write;
                    let sender_event = sent.before.steps.iter().position(|step| step.view == 10 && matches!(step.action, LocalAction::Send { label: 3, .. })).unwrap();
                    let receiver_event = received.before.steps.iter().position(|step| step.view == 4 && matches!(step.action, LocalAction::Recv { label: 3, .. })).unwrap();
                    let path = usize::from(holder - 1);
                    let other_path = if holder == 1 { 1 } else { 0 };
                    let header = sent.frame().header;
                    writeln!(proof, "def peerWitnessSenderBefore{holder} : ActorProgressObservation := {}\n\n\
                        def peerWitnessReceiverBefore{holder} : ActorProgressObservation := {}\n\n\
                        def peerWitnessReceiverAfter{holder} : ActorProgressObservation := {}\n\n\
                        def peerWitnessWire{holder} : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n\
                        def peerWitnessSender{holder} : SourceSendAdmission := {{ image := peerWitnessImage{holder}, base := peerWitnessSenderBefore{holder}, pathIndex := {path}, event := {sender_event}, wire := peerWitnessWire{holder} }}\n\n\
                        def peerWitnessReceiver{holder} (path : Nat) : ActorCommitRecord := {{ step := {{ pathIndex := path, event := {receiver_event}, view := 4, before := peerWitnessReceiverBefore{holder}, after := peerWitnessReceiverAfter{holder} }}, wire := peerWitnessWire{holder} }}\n\n\
                        theorem peerWitnessSender{holder}Checked : peerWitnessSender{holder}.check peerWitnessChoreo {} = true := by decide\n\n\
                        theorem peerWitnessReceiver{holder}CompatibleChecked : (peerWitnessReceiver{holder} {path}).check peerWitnessImage4 peerWitnessChoreo {} = true := by decide\n\n\
                        theorem peerWitnessReceiver{holder}OtherPathChecked : (peerWitnessReceiver{holder} {other_path}).check peerWitnessImage4 peerWitnessChoreo {} = true := by decide\n\n\
                        theorem peerWitnessPair{holder}CompatibleChecked : sourceReceivePairCheck peerWitnessSender{holder} (peerWitnessReceiver{holder} {path}) peerWitnessImage4 peerWitnessChoreo {} = true := by decide\n\n\
                        theorem peerWitnessPair{holder}OtherPathRejected : sourceReceivePairCheck peerWitnessSender{holder} (peerWitnessReceiver{holder} {other_path}) peerWitnessImage4 peerWitnessChoreo {} = false := by decide\n\n\
                        theorem peerWitnessPair{holder}UnknownPeerHasCoherentWitness : SourceReceivePair peerWitnessSender{holder} (peerWitnessReceiver{holder} {other_path}) peerWitnessImage4 peerWitnessChoreo {} :=\n  \
                          a_receive_pair_keeps_its_coherence_under_local_witness_changes\n    \
                            (sendPath := {path}) (receivePath := {other_path})\n    \
                            (a_concrete_receive_pair_has_coherent_local_witnesses peerWitnessPair{holder}CompatibleChecked) peerWitnessSender{holder}Checked peerWitnessReceiver{holder}OtherPathChecked\n",
                        super::commit::observation_source(&sent.before),
                        super::commit::observation_source(&received.before),
                        super::commit::observation_source(&received.after),
                        sid.raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(), sent.frame().payload(),
                        sid.raw(), sid.raw(), sid.raw(), sid.raw(), sid.raw(), sid.raw(),
                    ).unwrap();
                }
            }
        });
    #[cfg(not(miri))]
    {
        let directory =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("PeerWitnessGenerated.lean"), proof).unwrap();
    }
}
