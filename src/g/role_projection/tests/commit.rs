//! Observe real owned commits through the actor's sole progress and view records.
use super::*;
use crate::{
    endpoint::proof_snapshot::{ActorSnapshot, actor_snapshot},
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};
use std::vec::Vec;

type Steps = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Par<g::Send<10, 3, Read>, g::Send<1, 4, Value>>,
        g::Seq<g::Send<1, 0, Loan>, g::Par<g::Send<0, 5, OtherLoan>, g::Send<1, 6, ObserverLoan>>>,
    >,
>;
type ObserverLoan = g::Msg<5, Endpoint<'static, 12>>;

fn done(snapshot: &ActorSnapshot) -> Vec<bool> {
    snapshot.steps.iter().map(|step| step.done).collect()
}

fn resource(snapshot: &ActorSnapshot) -> (u32, u32) {
    let (_, run, nonce) = snapshot.views.iter().find(|view| view.0 == 10).unwrap();
    (*run, *nonce)
}

#[test]
fn owned_commits_publish_the_shared_actor_step_and_resource_view_together() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 16>::IMAGE_REF,
    );
    let peer_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Steps, 16>::IMAGE_REF,
    );
    // These ordinary co-resident views let the test read shared progress while
    // the completed send future still borrows its sender and owns its payload.
    // Their later owned sends fix the initial actors in the global itself.
    let auth_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    let observer_program = crate::global::role_program::role_program_from_image::<12>(
        &RoleProjection::<12, Steps, 16>::IMAGE_REF,
    );
    let auth_sink_program = crate::global::role_program::role_program_from_image::<5>(
        &RoleProjection::<5, Steps, 16>::IMAGE_REF,
    );
    let observer_sink_program = crate::global::role_program::role_program_from_image::<6>(
        &RoleProjection::<6, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut owner_slab = [0; 48 * 1024];
            let mut peer_slab = [0; 48 * 1024];
            let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
            let mut peer_storage = SessionKitStorage::<TestTransport>::uninit();
            let owner_rv = owner_storage
                .init()
                .rendezvous(&mut owner_slab, network.clone())
                .unwrap();
            let peer_rv = peer_storage
                .init()
                .rendezvous(&mut peer_slab, network)
                .unwrap();
            let sid = SessionId::new(874);
            let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
            let db = owner_rv.enter(sid, &db_program).unwrap();
            let mut worker = peer_rv.enter(sid, &worker_program).unwrap();
            let mut service = peer_rv.enter(sid, &service_program).unwrap();
            let mut peer = peer_rv.enter(sid, &peer_program).unwrap();
            let auth = owner_rv.enter(sid, &auth_program).unwrap();
            let observer = peer_rv.enter(sid, &observer_program).unwrap();
            let mut auth_sink = peer_rv.enter(sid, &auth_sink_program).unwrap();
            let mut observer_sink = peer_rv.enter(sid, &observer_sink_program).unwrap();

            // SAFETY: owner_rv attaches this actor with TestTransport.
            let owner_before = unsafe { actor_snapshot(&owner) };
            // SAFETY: db was attached through the same TestTransport owner_rv.
            assert_eq!(unsafe { actor_snapshot(&db) }, owner_before);
            let owner_sent = {
                let loan = owner.send::<Loan>(db);
                tokio::pin!(loan);
                loan.as_mut().await.unwrap();
                // Observe before Drop could mask a missing transfer retirement.
                // SAFETY: auth belongs to the TestTransport-backed owner actor.
                unsafe { actor_snapshot(&auth) }
            };
            assert_eq!(done(&owner_before), [false, false, false]);
            assert_eq!(done(&owner_sent), [true, false, false]);
            assert_eq!(owner_sent.counter, owner_before.counter);
            assert_eq!(owner_sent.live_views + 1, owner_before.live_views);
            assert_eq!(resource(&owner_sent), (0, resource(&owner_before).1 + 1));

            // SAFETY: peer_rv attaches this actor with TestTransport.
            let worker_before = unsafe { actor_snapshot(&worker) };
            let mut db = worker.recv::<Loan>().await.unwrap();
            // SAFETY: worker retains its TestTransport-backed peer actor.
            let worker_received = unsafe { actor_snapshot(&worker) };
            assert_eq!(done(&worker_before), [false, false, false, false, false]);
            assert_eq!(done(&worker_received), [true, false, false, false, false]);
            assert_eq!(worker_received.counter, worker_before.counter + 2);
            assert_eq!(worker_received.live_views, worker_before.live_views + 1);
            assert_eq!(
                resource(&worker_received),
                (worker_received.run, worker_received.counter)
            );
            // SAFETY: receiving the view binds it to worker's TestTransport actor.
            assert_eq!(unsafe { actor_snapshot(&db) }, worker_received);

            let (sent_db, sent_worker, read, value) = tokio::join!(
                db.send::<Read>(&7),
                worker.send::<Value>(&8),
                service.recv::<Read>(),
                peer.recv::<Value>(),
            );
            sent_db.unwrap();
            sent_worker.unwrap();
            assert_eq!(read.unwrap(), 7);
            assert_eq!(value.unwrap(), 8);
            // SAFETY: ordinary operations do not change the actor's transport.
            let worker_worked = unsafe { actor_snapshot(&worker) };
            assert_eq!(done(&worker_worked), [true, true, true, false, false]);
            assert_eq!(worker_worked.views, worker_received.views);
            assert_eq!(worker_worked.counter, worker_received.counter);

            let worker_returned = {
                let returned = worker.send::<Loan>(db);
                tokio::pin!(returned);
                returned.as_mut().await.unwrap();
                // SAFETY: observer was attached through the TestTransport peer_rv.
                unsafe { actor_snapshot(&observer) }
            };
            assert_eq!(done(&worker_returned), [true, true, true, true, false]);
            assert_eq!(worker_returned.counter, worker_worked.counter);
            assert_eq!(worker_returned.live_views + 1, worker_worked.live_views);
            assert_eq!(
                resource(&worker_returned),
                (0, resource(&worker_worked).1 + 1)
            );
            let db = owner.recv::<Loan>().await.unwrap();
            // SAFETY: owner retains its TestTransport-backed owner actor.
            let owner_received = unsafe { actor_snapshot(&owner) };
            assert_eq!(done(&owner_received), [true, true, false]);
            assert_eq!(owner_received.counter, owner_sent.counter + 2);
            assert_eq!(owner_received.live_views, owner_sent.live_views + 1);
            assert_eq!(
                resource(&owner_received),
                (owner_received.run, owner_received.counter)
            );
            // SAFETY: the returned view now belongs to owner's TestTransport actor.
            assert_eq!(unsafe { actor_snapshot(&db) }, owner_received);

            #[cfg(not(miri))]
            write_observations(&[
                ("ownerLoan", 0, &owner_before, &owner_sent),
                ("workerLoan", 1, &worker_before, &worker_received),
                ("workerReturn", 1, &worker_worked, &worker_returned),
                ("ownerReturn", 0, &owner_sent, &owner_received),
            ]);

            let (auth_sent, observer_sent, auth_received, observer_received) = tokio::join!(
                owner.send::<OtherLoan>(auth),
                worker.send::<ObserverLoan>(observer),
                auth_sink.recv::<OtherLoan>(),
                observer_sink.recv::<ObserverLoan>(),
            );
            auth_sent.unwrap();
            observer_sent.unwrap();
            drop(auth_received.unwrap());
            drop(observer_received.unwrap());
        });
}

#[cfg(not(miri))]
pub(super) fn observation_source(snapshot: &ActorSnapshot) -> std::string::String {
    let views = snapshot
        .views
        .iter()
        .map(|(role, run, nonce)| {
            std::format!("{{ role := {role}, run := {run}, nonce := {nonce} }}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let steps = snapshot
        .steps
        .iter()
        .map(|step| {
            let (eff, side) = match step.action {
                LocalAction::Send { eff_index, .. } => (eff_index, "send"),
                LocalAction::Recv { eff_index, .. } => (eff_index, "receive"),
                LocalAction::Local { eff_index, .. } => (eff_index, "local"),
                LocalAction::Terminate => unreachable!("observations contain only operation rows"),
            };
            std::format!(
                "{{ effIndex := {}, view := {}, side := .{side}, done := {} }}",
                eff.dense_ordinal(),
                step.view,
                step.done
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let choices = snapshot
        .choices
        .iter()
        .map(|(scope, arm)| {
            let arm = match arm {
                None => "none",
                Some(0) => "some .left",
                Some(1) => "some .right",
                _ => panic!("invalid observed route arm"),
            };
            std::format!("({scope}, {arm})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    std::format!(
        "{{ actor := {}, run := {}, counter := {}, liveViews := {}, views := [{views}], steps := [{steps}], choices := [{choices}] }}",
        snapshot.actor,
        snapshot.run,
        snapshot.counter,
        snapshot.live_views
    )
}

/// Serialize copied evidence only. All Endpoint operations remain above.
#[cfg(not(miri))]
fn write_observations(observations: &[(&str, u8, &ActorSnapshot, &ActorSnapshot)]) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    let owner_image = &RoleProjection::<0, Steps, 16>::IMAGE_REF;
    let worker_image = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
    let mut source = std::format!(
        "import Hibana.ActorCommit\n\nset_option maxRecDepth 10000\n\n\
         def actorCommitChoreo : Hibana.Choreo := {}\n\n\
         def ownerImage : Hibana.RustDescriptorImage := {}\n\n\
         def workerImage : Hibana.RustDescriptorImage := {}\n\n",
        Steps::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(owner_image)),
        descriptor_source(RoleDescriptorRef::from_resident(worker_image)),
    );
    for (name, view, before, after) in observations {
        let (image, image_name) = match before.actor {
            0 => (owner_image, "ownerImage"),
            1 => (worker_image, "workerImage"),
            _ => panic!("this qualification fixture has only two observed actors"),
        };
        for snapshot in [before, after] {
            assert_eq!(snapshot.actor, image.role());
            assert_eq!(snapshot.steps.len(), image.local_step_count());
            for (index, step) in snapshot.steps.iter().enumerate() {
                assert_eq!(step.action, image.local_step_node(index).unwrap().action());
            }
        }
        let changes = before
            .steps
            .iter()
            .zip(&after.steps)
            .enumerate()
            .filter_map(|(index, (old, new))| (old.done != new.done).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(changes.len(), 1);
        let event = changes[0];
        assert!(!before.steps[event].done && after.steps[event].done);
        assert_eq!(after.steps[event].view, *view);
        let wrong_event = (event + 1) % before.steps.len();
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorCommitObservation := {{\n  \
             pathIndex := 0\n  event := {event}\n  view := {view}\n  \
             before := {}\n  after := {}\n}}\n\n\
             theorem {name}Accepted :\n  \
             {name}.check {image_name} actorCommitChoreo = true := by\n  decide\n\n\
             theorem {name}SourceOrdered :\n  \
             {name}.SourceOrdered {image_name} actorCommitChoreo :=\n  \
             Hibana.accepted_actor_commit_matches_source_progress {name}Accepted\n\n\
             theorem {name}MissingProgressRejected :\n  \
             ({{ {name} with after := {{ {name}.after with steps := {name}.before.steps }} }} : \
             Hibana.ActorCommitObservation).check {image_name} actorCommitChoreo = false := by\n  decide\n\n\
             theorem {name}MissingViewRejected :\n  \
             ({{ {name} with after := {{ {name}.after with views := {name}.before.views }} }} : \
             Hibana.ActorCommitObservation).check {image_name} actorCommitChoreo = false := by\n  decide\n\n\
             theorem {name}WrongLocalRowRejected :\n  \
             ({{ {name} with event := {wrong_event} }} : Hibana.ActorCommitObservation).check \
             {image_name} actorCommitChoreo = false := by\n  decide\n\n",
            observation_source(before), observation_source(after),
        ));
    }
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ActorCommitGenerated.lean"), source).unwrap();
}
