//! Complete actor-family observations of real source-enabled operations.
//! The append-only cuts are never read by localsides to authorize progress.
use super::*;
use crate::{
    endpoint::proof_snapshot::{ActorSnapshot, actor_admissions, actor_snapshot},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use std::{vec, vec::Vec};

type Steps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<g::Par<g::Send<10, 3, Read>, g::Send<1, 4, Value>>, g::Send<1, 0, Loan>>,
>;

struct Cut {
    name: &'static str,
    actors: [(ActorSnapshot, Vec<bool>); 4],
    accepted: Vec<usize>,
}

#[test]
fn a_complete_actor_visit_exposes_real_parallel_admissions_and_completion() {
    // The transport is entirely in memory. Use Tokio's executor without an
    // OS I/O driver so Miri can execute the same concurrent localsides.
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
            let owner_program = crate::global::role_program::role_program_from_image::<0>(
                &RoleProjection::<0, Steps, 8>::IMAGE_REF,
            );
            let worker_program = crate::global::role_program::role_program_from_image::<1>(
                &RoleProjection::<1, Steps, 8>::IMAGE_REF,
            );
            let db_program = crate::global::role_program::role_program_from_image::<10>(
                &RoleProjection::<10, Steps, 8>::IMAGE_REF,
            );
            let reader_program = crate::global::role_program::role_program_from_image::<3>(
                &RoleProjection::<3, Steps, 8>::IMAGE_REF,
            );
            let peer_program = crate::global::role_program::role_program_from_image::<4>(
                &RoleProjection::<4, Steps, 8>::IMAGE_REF,
            );
            let network = TestTransport::new();
            let mut owner_slab = [0; 32 * 1024];
            let mut peer_slab = [0; 48 * 1024];
            let mut owner_storage = SessionKitStorage::uninit();
            let mut peer_storage = SessionKitStorage::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let peer_rv = peer_storage
                .init()
                .rendezvous(&mut peer_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9054);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = peer_rv.enter(sid, &worker_program).unwrap();
            let mut reader = peer_rv.enter(sid, &reader_program).unwrap();
            let mut peer = peer_rv.enter(sid, &peer_program).unwrap();
            let mut cuts = Vec::new();
            let mut wires = Vec::new();

            // SAFETY: all four live endpoints retain their pinned TestTransport actors.
            // Copy only read-only evidence under each existing short operation lease.
            cuts.push(Cut {
                name: "visitInitial",
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![],
            });

            owner.send::<Loan>(db).await.unwrap();
            wires.push(network.queued_frame(1));
            assert_eq!(wires[0].1, [10, 0, 0]);
            cuts.push(Cut {
                name: "visitLoanAccepted",
                // SAFETY: the same four TestTransport actors remain pinned.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0],
            });

            let mut db = worker.recv::<Loan>().await.unwrap();
            cuts.push(Cut {
                name: "visitLoanReceived",
                // SAFETY: receipt preserves the concrete actor types and allocation.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0],
            });
            // The global's two independent sends are both admitted before either
            // runs, then driven as concurrent futures on distinct affine views.
            assert_eq!(cuts.last().unwrap().actors[1].1, [false, true, true, false]);
            let (read, value) = tokio::join!(db.send::<Read>(&17), worker.send::<Value>(&19));
            read.unwrap();
            value.unwrap();
            wires.push(network.queued_frame(3));
            wires.push(network.queued_frame(4));
            cuts.push(Cut {
                name: "visitParallelAccepted",
                // SAFETY: both send futures released their short actor borrows.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0, 1, 2],
            });
            let (read, value) = tokio::join!(reader.recv::<Read>(), peer.recv::<Value>());
            assert_eq!(read.unwrap(), 17);
            assert_eq!(value.unwrap(), 19);
            cuts.push(Cut {
                name: "visitParallelReceived",
                // SAFETY: both receive operations finished before copying progress.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0, 1, 2],
            });

            worker.send::<Loan>(db).await.unwrap();
            wires.push(network.queued_frame(0));
            assert_eq!(wires[3].1, [10, 0, 3]);
            cuts.push(Cut {
                name: "visitReturnAccepted",
                // SAFETY: WORKER's own view retains its actor after the owned move.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0, 1, 2, 3],
            });
            let db = owner.recv::<Loan>().await.unwrap();
            cuts.push(Cut {
                name: "visitComplete",
                // SAFETY: the returned continuation and all actor records are live.
                actors: unsafe {
                    [
                        (actor_snapshot(&owner), actor_admissions(&owner)),
                        (actor_snapshot(&worker), actor_admissions(&worker)),
                        (actor_snapshot(&reader), actor_admissions(&reader)),
                        (actor_snapshot(&peer), actor_admissions(&peer)),
                    ]
                },
                accepted: vec![0, 1, 2, 3],
            });
            assert!(
                cuts.last()
                    .unwrap()
                    .actors
                    .iter()
                    .all(|(snapshot, admitted)| {
                        snapshot.steps.iter().all(|step| step.done)
                            && admitted.iter().all(|&ready| !ready)
                    })
            );
            for (cut, expected) in cuts.iter().zip([0, 1, 1, 3, 3, 4, 4]) {
                assert_eq!(cut.accepted.len(), expected);
                assert_eq!(
                    cut.actors.each_ref().map(|(actor, _)| actor.actor),
                    [0, 1, 3, 4]
                );
                std::eprintln!("{}: accepted={:?}", cut.name, cut.accepted);
            }
            #[cfg(not(miri))]
            write_observations(&cuts, &wires);
            drop(db);
        });
}

/// Encode copied evidence. No Endpoint operation or authorization lives here.
#[cfg(not(miri))]
fn write_observations(cuts: &[Cut], wires: &[(crate::transport::FrameHeader, Vec<u8>)]) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let images = [
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
        &RoleProjection::<3, Steps, 8>::IMAGE_REF,
        &RoleProjection::<4, Steps, 8>::IMAGE_REF,
    ];
    let mut source = std::format!(
        "import Hibana.ActorVisitProgress\n\nset_option maxRecDepth 10000\n\n\
         def visitChoreo : Hibana.Choreo := {}\n\n\
         def visitSource : Hibana.EndpointDelegation.Source := visitChoreo.ownershipSource?.get (by decide)\n\n\
         def visitPath : Hibana.EndpointDelegation.NormalizedPath :=\n  \
         (((Hibana.EndpointDelegation.normalize visitSource).get (by decide)).paths[0]?).get (by decide)\n\n",
        Steps::lean_source(),
    );
    for (index, image) in images.into_iter().enumerate() {
        source.push_str(&std::format!(
            "def visitImage{index} : Hibana.RustDescriptorImage := {}\n\n",
            descriptor_source(RoleDescriptorRef::from_resident(image)),
        ));
    }
    for (index, (header, payload)) in wires.iter().enumerate() {
        source.push_str(&std::format!(
            "def visitAcceptedWire{index} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {payload:?} }}\n\n",
            header.session().raw(), header.lane(), header.source_role(), header.target_role(), header.label().raw(),
        ));
    }
    for cut in cuts {
        for (index, (snapshot, admitted)) in cut.actors.iter().enumerate() {
            source.push_str(&std::format!(
                "def {}Actor{index} : Hibana.ActorVisitRecord := {{ image := visitImage{index}, \
                 admission := {{ snapshot := {}, pathIndex := 0, admitted := {admitted:?} }} }}\n\n",
                cut.name, super::commit::observation_source(snapshot),
            ));
        }
        source.push_str(&std::format!(
            "def {name} : List Hibana.ActorVisitRecord := [{name}Actor0, {name}Actor1, {name}Actor2, {name}Actor3]\n\n\
             def {name}Accepted : List Nat := {accepted:?}\n\n\
             theorem {name}Checked : Hibana.actorVisitCutCheck {name} visitChoreo visitSource visitPath {name}Accepted = true := by decide\n\n",
            name = cut.name, accepted = cut.accepted,
        ));
        if cut.name != "visitComplete" {
            source.push_str(&std::format!(
                "theorem {name}HasActualAdmissionOrPublication :\n  \
                 ∃ point record data row,\n    \
                 point ∈ Hibana.sourceVisitPoints visitPath.bindings ∧\n    \
                 Hibana.actorVisitObserved {name} visitChoreo visitPath {name}Accepted point = false ∧\n    \
                 record ∈ {name} ∧ record.image.decodeActorOrder? visitChoreo = some data ∧ row ∈ data.activeRows visitPath ∧\n    \
                 ((point = .committed row.actionId ∧ row.actionId.side = .send ∧\n      \
                 Hibana.actorVisitObserved {name} visitChoreo visitPath {name}Accepted (.accepted row.actionId.event) = true ∧\n      \
                 record.admission.snapshot.doneState.done row.eventId = false) ∨\n      \
                 (point = row.actionId.entryPoint ∧ Hibana.sourceRowReady data visitPath record.admission.snapshot row = true ∧\n      \
                 record.admission.admitted[row.eventId]? = some true ∧\n      \
                 Hibana.sourceRowViewsIssued visitPath record.admission.snapshot row = true)) :=\n  \
                 Hibana.checked_unfinished_actor_visit_has_admission_or_publication {name}Checked\n    \
                 (by decide : ∃ point ∈ Hibana.sourceVisitPoints visitPath.bindings, \
                 Hibana.actorVisitObserved {name} visitChoreo visitPath {name}Accepted point = false)\n\n",
                name = cut.name,
            ));
        }
    }
    source.push_str(r#"theorem visitCompleteHasNoUnfinishedPoint :
  (Hibana.sourceVisitPoints visitPath.bindings).all
    (Hibana.actorVisitObserved visitComplete visitChoreo visitPath visitCompleteAccepted) = true := by decide

theorem bothParallelSendsHaveActualAdmission :
  visitLoanReceivedActor1.admission.admitted[1]? = some true ∧
    visitLoanReceivedActor1.admission.admitted[2]? = some true ∧
    Hibana.actorVisitObserved visitLoanReceived visitChoreo visitPath visitLoanReceivedAccepted (.accepted 1) = false ∧
    Hibana.actorVisitObserved visitLoanReceived visitChoreo visitPath visitLoanReceivedAccepted (.accepted 2) = false := by decide

theorem missingActorIsRejected :
  Hibana.actorVisitCutCheck (visitLoanReceived.take 2) visitChoreo visitSource visitPath visitLoanReceivedAccepted = false := by decide

theorem duplicateActorIsRejected :
  Hibana.actorVisitCutCheck (visitLoanReceived ++ [visitLoanReceivedActor1]) visitChoreo visitSource visitPath visitLoanReceivedAccepted = false := by decide

theorem missingAcceptedSendIsRejected :
  Hibana.actorVisitCutCheck visitLoanAccepted visitChoreo visitSource visitPath [] = false := by decide

theorem unknownAcceptedEventIsRejected :
  Hibana.actorVisitCutCheck visitLoanAccepted visitChoreo visitSource visitPath [0, 99] = false := by decide

theorem duplicateAcceptedEventIsRejected :
  Hibana.actorVisitCutCheck visitLoanAccepted visitChoreo visitSource visitPath [0, 0] = false := by decide

theorem fabricatedEmptySourceDomainIsRejected :
  Hibana.actorVisitCutCheck [] visitChoreo visitSource
    { selections := [], bindings := [] } [] = false := by decide

-- Model-only interleavings using copied progress, not an extra claim that
-- these mixed snapshots were captured from a single real callback cut.
theorem modelAcceptedBeforePublicationIsAllowed :
  Hibana.actorVisitCutCheck visitInitial visitChoreo visitSource visitPath [0] = true := by decide

theorem modelAcceptedBeforePublicationIsNotAnotherSend :
  Hibana.actorVisitObserved visitInitial visitChoreo visitPath [0] (.accepted 0) = true ∧
    Hibana.actorVisitObserved visitInitial visitChoreo visitPath [0] (.committed ⟨0, .send⟩) = false := by decide

theorem modelReceiptBeforeSenderPublicationIsAllowed :
  Hibana.actorVisitCutCheck
    [visitInitialActor0, visitLoanReceivedActor1, visitInitialActor2, visitInitialActor3]
    visitChoreo visitSource visitPath [0] = true := by decide
"#);
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ActorVisitProgressGenerated.lean"), source).unwrap();
}
