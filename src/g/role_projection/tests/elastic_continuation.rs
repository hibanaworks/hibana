use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

type Start = g::Msg<201, u32>;
type Ack = g::Msg<202, u32>;
type Finish = g::Msg<203, u32>;
type Body = g::Seq<g::Send<0, 1, Start>, g::Send<1, 0, Ack>>;
type Steps = g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>;
type Stop = g::Msg<204, u32>;
type OtherAck = g::Msg<208, u32>;
type Final = g::Msg<209, u32>;
type BranchBody =
    g::Seq<g::Send<0, 1, Start>, g::Route<g::Send<1, 0, Ack>, g::Send<1, 0, OtherAck>>>;
type BranchSteps = g::Seq<
    g::Roll<BranchBody>,
    g::Seq<g::Route<g::Send<1, 0, Finish>, g::Send<1, 0, Stop>>, g::Send<0, 1, Final>>,
>;

#[test]
fn a_queued_old_continuation_survives_a_new_body_visit() {
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let a_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let b_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut a_slab = [0; 16 * 1024];
            let mut b_slab = [0; 16 * 1024];
            let mut a_storage = SessionKitStorage::uninit();
            let mut b_storage = SessionKitStorage::uninit();
            let a_rv = a_storage
                .init()
                .rendezvous(&mut a_slab, network.clone())
                .unwrap();
            let b_rv = b_storage
                .init()
                .rendezvous(&mut b_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9072);
            let mut a = a_rv.enter(sid, &a_program).unwrap();
            let mut b = b_rv.enter(sid, &b_program).unwrap();
            tokio::time::timeout(core::time::Duration::from_millis(100), async {
                a.send::<Start>(&7).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                b.send::<Ack>(&8).await.unwrap();
                assert_eq!(a.recv::<Ack>().await.unwrap(), 8);
                b.send::<Finish>(&9).await.unwrap();
                a.send::<Start>(&10).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                b.send::<Ack>(&11).await.unwrap();
                // SAFETY: a retains its pinned TestTransport actor and no
                // operation is polled between these read-only observations.
                let before = unsafe { crate::endpoint::proof_snapshot::actor_snapshot(&a) };
                let admitted = unsafe { crate::endpoint::proof_snapshot::actor_admissions(&a) };
                assert_eq!(before.retained_rolls, [0]);
                assert_eq!(admitted, [false, true, true]);
                assert_eq!(a.recv::<Finish>().await.unwrap(), 9);
                assert_eq!(a.recv::<Ack>().await.unwrap(), 11);
                a.send::<Start>(&12).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 12);
                b.send::<Ack>(&13).await.unwrap();
                assert_eq!(a.recv::<Ack>().await.unwrap(), 13);
            })
            .await
            .expect("an accepted old continuation must not strand the new visit");
            assert!(network.queue_is_empty());
        });
}

/// Serialize read-only evidence; all Endpoint operations remain above.
#[cfg(not(miri))]
fn write_retained_admission(
    projected: &'static RoleImageRef,
    snapshot: &crate::endpoint::proof_snapshot::ActorSnapshot,
    admitted: &[bool],
    after: &crate::endpoint::proof_snapshot::ActorSnapshot,
    wire: &crate::test_support::transport::FrameEvidence,
) {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    use std::string::ToString;
    let image = descriptor_source(RoleDescriptorRef::from_resident(projected));
    let choreo = BranchSteps::lean_source();
    let state = super::commit::observation_source(snapshot);
    let after_state = super::commit::observation_source(after);
    let before_writes = snapshot.completion_mutations;
    let after_writes = after.completion_mutations;
    let header = wire.header;
    let wire_source = std::format!(
        "{{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}",
        header.session().raw(),
        header.lane(),
        header.source_role(),
        header.target_role(),
        header.label().raw(),
        wire.payload(),
    );
    let admitted = admitted
        .iter()
        .map(bool::to_string)
        .collect::<std::vec::Vec<_>>()
        .join(", ");
    let retained = snapshot
        .retained_rolls
        .iter()
        .map(u16::to_string)
        .collect::<std::vec::Vec<_>>()
        .join(", ");
    let source = std::format!(
        r#"import Hibana.SourceOccurrenceTrace

set_option maxRecDepth 10000
def elasticChoreo : Hibana.Choreo := {choreo}
def elasticImage : Hibana.RustDescriptorImage := {image}
def elasticSnapshot : Hibana.ActorProgressObservation := {state}
def elasticAfter : Hibana.ActorProgressObservation := {after_state}
def elasticCandidate : Hibana.ActorAdmissionObservation := {{
  snapshot := elasticSnapshot, pathIndex := 2, admitted := [{admitted}], retainedRolls := [{retained}]
}}
def elasticData : Hibana.DecodedActorOrder := (elasticImage.decodeActorOrder? elasticChoreo).get (by decide)
def elasticPath : Hibana.EndpointDelegation.NormalizedPath := elasticData.normalized.paths[2]'(by decide)
def elasticFinish : Hibana.DecodedActorRow := elasticData.rows[3]'(by decide)

theorem actualLateCandidateIsChecked : elasticCandidate.candidateCheck elasticImage elasticChoreo 2 = true := by decide
theorem currentBitsCannotSupplyTheOldAck :
    Hibana.sourceRowReady elasticData elasticPath elasticSnapshot elasticFinish = false := by decide
theorem retainedVisitSuppliesTheLateReceive :
    Hibana.sourceCandidateRowReady elasticImage elasticChoreo elasticCandidate.retainedRolls
      elasticData elasticPath elasticSnapshot elasticFinish = true := by decide
theorem forgettingTheRetainedVisitIsRejected :
    ({{ elasticCandidate with retainedRolls := [] }} : Hibana.ActorAdmissionObservation).candidateCheck
      elasticImage elasticChoreo 2 = false := by decide
theorem anUnknownRetainedScopeIsRejected :
    ({{ elasticCandidate with retainedRolls := [65535] }} : Hibana.ActorAdmissionObservation).candidateCheck
      elasticImage elasticChoreo 2 = false := by decide
theorem aRepeatedRetainedScopeIsRejected :
    ({{ elasticCandidate with retainedRolls := [0, 0] }} : Hibana.ActorAdmissionObservation).candidateCheck
      elasticImage elasticChoreo 2 = false := by decide
theorem anOldVisitCannotSupplyTheFreshStart :
    Hibana.sourceCandidateRowReady elasticImage elasticChoreo elasticCandidate.retainedRolls
      elasticData elasticPath
      {{ elasticSnapshot with steps := elasticSnapshot.steps.set 0 {{ elasticSnapshot.steps[0]'(by decide) with done := false }} }}
      (elasticData.rows[2]'(by decide)) = false := by decide
def elasticSource : Hibana.EndpointDelegation.Source := elasticChoreo.ownershipSource?.get (by decide)
def elasticStartMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[0]'(by decide) |>.2
def elasticAckMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[1]'(by decide) |>.2
def elasticOtherMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[2]'(by decide) |>.2
def elasticFinishMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[3]'(by decide) |>.2
def elasticStopMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[4]'(by decide) |>.2
def elasticFinalMessage : Hibana.EndpointDelegation.Message := (elasticSource.messageEntries 0)[5]'(by decide) |>.2
def elasticBody : Hibana.EndpointDelegation.Source := .seq (.send elasticStartMessage)
  (.route none (.send elasticAckMessage) (.send elasticOtherMessage))
def elasticContinuation : Hibana.EndpointDelegation.Source := .seq
  (.route none (.send elasticFinishMessage) (.send elasticStopMessage)) (.send elasticFinalMessage)
def elasticExpanded : Hibana.EndpointDelegation.Source := .seq elasticBody (.par elasticBody elasticContinuation)
def elasticOriginalNormalization : Hibana.EndpointDelegation.Normalization :=
  (Hibana.EndpointDelegation.normalize elasticSource).get (by decide)
def elasticExpandedNormalization : Hibana.EndpointDelegation.Normalization :=
  (Hibana.EndpointDelegation.normalize elasticExpanded).get (by decide)
def elasticPlan : Hibana.SourceOccurrenceCut elasticSource := {{
  original := elasticOriginalNormalization, originalAccepted := by rfl
  expanded := elasticExpanded
  unfolding := .resumeAlongsideContinuation (.rollOnce (.oneCopy elasticBody))
    (.rollOnce (.oneCopy elasticBody)) (.oneCopy elasticContinuation)
  normalized := elasticExpandedNormalization, accepted := by rfl
  sameIssuance := by rfl
  path := elasticExpandedNormalization.paths[2]'(by decide), selected := by decide
  observed := fun _ => false
}}
def elasticPoints : List Hibana.SourcePoint := [
  .accepted 0, .committed ⟨0, .send⟩, .committed ⟨0, .receive⟩,
  .accepted 1, .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩,
  .accepted 6, .committed ⟨6, .send⟩,
  .accepted 3, .committed ⟨3, .send⟩, .committed ⟨3, .receive⟩,
  .accepted 5, .committed ⟨5, .send⟩]
def elasticTarget : Hibana.EndpointDelegation.ActionId := ⟨6, .receive⟩

theorem theActualPastSupportMatchesItsCausalOccurrences :
    elasticPlan.candidateHistoryMatches elasticImage elasticChoreo 2 elasticData elasticPath elasticFinish
      elasticSnapshot elasticPoints elasticTarget (retained := elasticCandidate.retainedRolls) = true := by decide
theorem interpretingTheCurrentBranchAsThePastIsRejected :
    elasticPlan.candidateHistoryMatches elasticImage elasticChoreo 2 elasticData elasticPath elasticFinish
      elasticSnapshot elasticPoints elasticTarget = false := by decide
theorem theLateReceiveHasActualRawAdmission : elasticCandidate.admitted[3]? = some true :=
  (Hibana.a_projected_current_occurrence_has_actual_candidate_admission actualLateCandidateIsChecked
    (data := elasticData) (path := elasticPath) (row := elasticFinish)
    (plan := elasticPlan) (points := elasticPoints) (target := elasticTarget)
    (by rfl) (by rfl) (by decide) (by decide) (by rfl) theActualPastSupportMatchesItsCausalOccurrences
    (by decide) (by decide)).2
def elasticReceive : Hibana.ActorCommitObservation := {{
  pathIndex := 2, event := 3, view := 0, before := elasticSnapshot, after := elasticAfter
}}
theorem theActualLateReceiveHasItsPhysicalUpdate :
    elasticReceive.transitionCheck elasticImage elasticChoreo = true := by decide
theorem theCurrentVisitCommitModelRejectsThatActualUpdate :
    elasticReceive.check elasticImage elasticChoreo = false := by decide
theorem aPhysicalUpdateCannotSilentlyFinishTheNewBody :
    ({{ elasticReceive with after := {{ elasticAfter with
      steps := elasticAfter.steps.set 2 {{ elasticAfter.steps[2]'(by decide) with done := true }} }} }} :
      Hibana.ActorCommitObservation).transitionCheck elasticImage elasticChoreo = false := by decide
def elasticReceiveRecord : Hibana.ActorCommitRecord := {{
  step := elasticReceive
  wire := {wire_source}
}}
theorem theLateReceiveHasItsActualMeasuredTransition :
    Hibana.ActorMeasuredTransition elasticImage elasticChoreo 9073
      (elasticSnapshot, {before_writes}) (elasticAfter, {after_writes}) [] elasticReceiveRecord :=
  .ordinary (record := {{ commit := elasticReceiveRecord, before := {before_writes}, after := {after_writes} }})
    (by decide) (by decide)
"#
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("ElasticContinuationGenerated.lean"), source).unwrap();
}

#[test]
fn a_queued_old_branch_does_not_skip_the_fresh_body_receive() {
    let () = g::ProgramProjection::<BranchSteps, 32>::VALIDATION;
    let a_program = crate::runtime::program::project::<0>(&g::Program::<BranchSteps>::new());
    let b_program = crate::runtime::program::project::<1>(&g::Program::<BranchSteps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for expected_label in [203, 204] {
                let network = TestTransport::new();
                let mut a_slab = [0; 16 * 1024];
                let mut b_slab = [0; 16 * 1024];
                let mut a_storage = SessionKitStorage::uninit();
                let mut b_storage = SessionKitStorage::uninit();
                let a_rv = a_storage
                    .init()
                    .rendezvous(&mut a_slab, network.clone())
                    .unwrap();
                let b_rv = b_storage
                    .init()
                    .rendezvous(&mut b_slab, network.clone())
                    .unwrap();
                let sid = SessionId::new(9073);
                let mut a = a_rv.enter(sid, &a_program).unwrap();
                let mut b = b_rv.enter(sid, &b_program).unwrap();
                tokio::time::timeout(core::time::Duration::from_millis(100), async {
                    a.send::<Start>(&7).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                    b.send::<Ack>(&8).await.unwrap();
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), 202);
                    assert_eq!(branch.recv::<Ack>().await.unwrap(), 8);
                    match expected_label {
                        203 => b.send::<Finish>(&9).await.unwrap(),
                        204 => b.send::<Stop>(&9).await.unwrap(),
                        _ => unreachable!(),
                    }
                    a.send::<Start>(&10).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                    b.send::<OtherAck>(&11).await.unwrap();
                    // SAFETY: a owns this pinned TestTransport actor; no poll
                    // occurs between the two copied admission observations.
                    let before = unsafe { crate::endpoint::proof_snapshot::actor_snapshot(&a) };
                    let admitted = unsafe { crate::endpoint::proof_snapshot::actor_admissions(&a) };
                    assert_eq!(before.retained_rolls, [0]);
                    assert!(admitted[2] && admitted[3]);
                    #[cfg(not(miri))]
                    let wire = network.accepted_frame(0, 0);
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), expected_label);
                    match branch.label() {
                        203 => assert_eq!(branch.recv::<Finish>().await.unwrap(), 9),
                        204 => assert_eq!(branch.recv::<Stop>().await.unwrap(), 9),
                        _ => unreachable!(),
                    }
                    // SAFETY: the completed branch released its local borrow;
                    // no further operation is polled during this observation.
                    let after = unsafe { crate::endpoint::proof_snapshot::actor_snapshot(&a) };
                    assert_eq!(after.retained_rolls, before.retained_rolls);
                    assert_eq!(after.completion_mutations, before.completion_mutations + 1);
                    assert!(!after.steps[2].done);
                    #[cfg(not(miri))]
                    if expected_label == 203 {
                        write_retained_admission(
                            a_program.role_image_ref(),
                            &before,
                            &admitted,
                            &after,
                            &wire,
                        );
                    }
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), 208);
                    assert_eq!(branch.recv::<OtherAck>().await.unwrap(), 11);
                    a.send::<Final>(&14).await.unwrap();
                    assert_eq!(b.recv::<Final>().await.unwrap(), 14);
                })
                .await
                .expect("an earlier branch cannot hide the fresh body head");
                assert!(network.queue_is_empty());
            }
        });
}

#[test]
fn retained_completion_never_supplies_a_fresh_body_predecessor() {
    let image = &RoleProjection::<0, Steps, 8>::IMAGE_REF;
    let roll = image.roll_scope_row(0).unwrap().scope();
    with_cursor(image, |cursor| {
        for index in 0..2 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        let reset = cursor.prepare_roll_reset(roll, None, &mut |_| None);
        cursor.clear_reentry_scope_events(reset);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_ok()
        );
        assert!(
            cursor
                .event_enabled(1, event(image, 1), &mut |_, _| None)
                .is_err()
        );
    });
}

#[test]
fn restarting_an_outer_visit_discards_its_old_inner_completion() {
    type Nested = g::Roll<g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>>;
    let () = g::ProgramProjection::<Nested, 8>::VALIDATION;
    let image = &RoleProjection::<0, Nested, 8>::IMAGE_REF;
    let scopes = [
        image.roll_scope_row(0).unwrap(),
        image.roll_scope_row(1).unwrap(),
    ];
    let inner = scopes
        .iter()
        .find(|row| row.event_row().len() == 2)
        .unwrap()
        .scope();
    let outer = scopes
        .iter()
        .find(|row| row.event_row().len() == 3)
        .unwrap()
        .scope();
    with_cursor(image, |cursor| {
        for index in 0..2 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        let reset = cursor.prepare_roll_reset(inner, None, &mut |_| None);
        cursor.clear_reentry_scope_events(reset);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_ok()
        );
        for index in 0..3 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        let reset = cursor.prepare_roll_reset(outer, None, &mut |_| None);
        cursor.clear_reentry_scope_events(reset);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_err()
        );
    });
}

#[test]
fn retained_roll_completion_has_storage_past_a_full_event_word() {
    type Tail = g::Send<0, 1, g::Msg<205, ()>>;
    type Two = g::Seq<Tail, Tail>;
    type Four = g::Seq<Two, Two>;
    type Eight = g::Seq<Four, Four>;
    type Sixteen = g::Seq<Eight, Eight>;
    type Boundary = g::Seq<
        g::Roll<g::Send<0, 1, g::Msg<206, ()>>>,
        g::Seq<Sixteen, g::Seq<Eight, g::Seq<Four, g::Seq<Two, Tail>>>>,
    >;
    let () = g::ProgramProjection::<Boundary, 128>::VALIDATION;
    let image = &RoleProjection::<0, Boundary, 128>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 32);
    assert_eq!(image.footprint().completion_bit_count(), 33);
    with_cursor(image, |cursor| {
        let commit = cursor
            .event_enabled(0, event(image, 0), &mut |_, _| None)
            .unwrap();
        cursor.advance_lane_to_relocatable_step(commit.progress_step());
        let reset =
            cursor.prepare_roll_reset(image.roll_scope_row(0).unwrap().scope(), None, &mut |_| {
                None
            });
        cursor.clear_reentry_scope_events(reset);
        assert!(cursor.has_retained_roll_completion());
        for index in 0..32 {
            let lane = image.local_step_lane(index).unwrap();
            assert!(!cursor.node_event_done_for_lane(index, lane));
        }
        assert!(
            cursor
                .event_enabled(1, event(image, 1), &mut |_, _| None)
                .is_err(),
            "a retained visit cannot authorize a new outgoing acceptance"
        );
    });
}

#[test]
fn a_late_branch_is_available_while_the_new_body_head_is_ordinary() {
    type Stop = g::Msg<204, u32>;
    type BranchSteps = g::Seq<g::Roll<Body>, g::Route<g::Send<1, 0, Finish>, g::Send<1, 0, Stop>>>;
    let () = g::ProgramProjection::<BranchSteps, 16>::VALIDATION;
    let a_program = crate::runtime::program::project::<0>(&g::Program::<BranchSteps>::new());
    let b_program = crate::runtime::program::project::<1>(&g::Program::<BranchSteps>::new());
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for expected_label in [203, 204] {
                let network = TestTransport::new();
                let mut a_slab = [0; 16 * 1024];
                let mut b_slab = [0; 16 * 1024];
                let mut a_storage = SessionKitStorage::uninit();
                let mut b_storage = SessionKitStorage::uninit();
                let a_rv = a_storage
                    .init()
                    .rendezvous(&mut a_slab, network.clone())
                    .unwrap();
                let b_rv = b_storage
                    .init()
                    .rendezvous(&mut b_slab, network.clone())
                    .unwrap();
                let sid = SessionId::new(9074);
                let mut a = a_rv.enter(sid, &a_program).unwrap();
                let mut b = b_rv.enter(sid, &b_program).unwrap();
                tokio::time::timeout(core::time::Duration::from_millis(100), async {
                    a.send::<Start>(&7).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                    b.send::<Ack>(&8).await.unwrap();
                    assert_eq!(a.recv::<Ack>().await.unwrap(), 8);
                    match expected_label {
                        203 => b.send::<Finish>(&9).await.unwrap(),
                        204 => b.send::<Stop>(&9).await.unwrap(),
                        _ => unreachable!(),
                    }
                    a.send::<Start>(&10).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                    b.send::<Ack>(&11).await.unwrap();
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), expected_label);
                    match branch.label() {
                        203 => assert_eq!(branch.recv::<Finish>().await.unwrap(), 9),
                        204 => assert_eq!(branch.recv::<Stop>().await.unwrap(), 9),
                        _ => unreachable!(),
                    }
                    assert_eq!(a.recv::<Ack>().await.unwrap(), 11);
                })
                .await
                .expect("a late branch remains observable beside an ordinary body head");
                assert!(network.queue_is_empty());
            }
        });
}
