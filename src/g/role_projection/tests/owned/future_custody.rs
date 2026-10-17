//! Serialize copied observations of the real Endpoint schedule.
//! Placement in a Rust variable, private future, or accepted frame is observed
//! by that schedule. These proof values never authorize a runtime operation.
use crate::endpoint::proof_snapshot::ActorSnapshot;
use crate::transport::FrameHeader;
use std::{string::String, vec::Vec};

pub(super) fn write(
    choreography: String,
    images: [String; 2],
    snapshots: &[(&str, &ActorSnapshot)],
    wires: &[(&str, &(FrameHeader, Vec<u8>))],
) {
    let mut source = std::format!(
        "import Hibana.OwnedCommit\n\nset_option maxRecDepth 10000\n\n\
         def custodyChoreo : Hibana.Choreo := {choreography}\n\n\
         def ownerImage : Hibana.RustDescriptorImage := {}\n\n\
         def workerImage : Hibana.RustDescriptorImage := {}\n\n",
        images[0],
        images[1],
    );
    for (name, snapshot) in snapshots {
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorProgressObservation := {}\n\n\
             theorem {name}Valid : {name}.valid = true := by decide\n\n",
            super::super::commit::observation_source(snapshot),
        ));
    }
    for (name, (header, payload)) in wires {
        source.push_str(&std::format!(
            "def {name} : Hibana.OwnedWireObservation := {{ session := {}, lane := {}, \
             sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\n",
            header.session().raw(),
            header.lane(),
            header.source_role(),
            header.target_role(),
            header.label().raw(),
            payload,
        ));
    }
    let snapshot = |name: &str| snapshots.iter().find(|row| row.0 == name).unwrap().1;
    for (name, before_name, after_name, view, image, wire) in [
        (
            "custodyLoanSend",
            "ownerPending",
            "ownerSent",
            0,
            "ownerImage",
            "loanWire",
        ),
        (
            "custodyLoanRecv",
            "workerBefore",
            "workerReceived",
            1,
            "workerImage",
            "loanWire",
        ),
        (
            "custodyReturnSend",
            "workerSealed",
            "workerSent",
            1,
            "workerImage",
            "returnWire",
        ),
        (
            "custodyReturnRecv",
            "ownerSent",
            "ownerReturned",
            11,
            "ownerImage",
            "returnWire",
        ),
    ] {
        let before = snapshot(before_name);
        let after = snapshot(after_name);
        let changed: Vec<_> = before
            .steps
            .iter()
            .zip(&after.steps)
            .enumerate()
            .filter_map(|(index, (old, new))| (old.done != new.done).then_some(index))
            .collect();
        assert_eq!(changed.len(), 1);
        let event = changed[0];
        assert!(!before.steps[event].done && after.steps[event].done);
        assert_eq!(after.steps[event].view, view);
        source.push_str(&std::format!(
            "def {name} : Hibana.ActorCommitObservation := {{ pathIndex := 0, event := {event}, \
             view := {view}, before := {before_name}, after := {after_name} }}\n\n\
             theorem {name}Accepted :\n  \
             {name}.ownedCheck {image} custodyChoreo {wire} {session} = true := by decide\n\n\
             theorem {name}SourceOrdered : {name}.SourceOrdered {image} custodyChoreo :=\n  \
             (Hibana.accepted_owned_commit_binds_source_view_and_wire {name}Accepted).1\n\n",
            session = wires[0].1.0.session().raw(),
        ));
    }
    for (name, observed) in [
        ("initialValue", "ownerBefore"),
        ("receivedValue", "workerReceived"),
        ("returnedValue", "ownerReturned"),
    ] {
        let actor = snapshot(observed);
        let view = actor.views.iter().find(|view| view.role == 10).unwrap();
        source.push_str(&std::format!(
            "def {name} : Nat × Nat × Nat × Nat := ({}, {}, {}, {})\n\n\
             theorem {name}Issued : {observed}.viewIssued 10 = true := by decide\n\n",
            actor.actor,
            view.role,
            view.run,
            view.nonce,
        ));
    }
    source.push_str(r#"open Hibana.EndpointTransfer

abbrev Custody := Observation (Nat × Nat × Nat × Nat) Hibana.OwnedWireObservation

def custodyInitial : Custody := initial initialValue
def custodySealed : Custody :=
  { usable := [], preparing := [initialValue], acceptedFutures := [], inFlight := [] }
def custodyPending : Custody := custodySealed
def custodySent : Custody :=
  { usable := [], preparing := [], acceptedFutures := [initialValue], inFlight := [loanWire] }
def custodyReceived : Custody :=
  { usable := [receivedValue], preparing := [], acceptedFutures := [initialValue], inFlight := [] }
def custodyReturnSealed : Custody :=
  { usable := [], preparing := [receivedValue], acceptedFutures := [initialValue], inFlight := [] }
def custodyReturnSent : Custody :=
  { usable := [], preparing := [], acceptedFutures := [receivedValue, initialValue], inFlight := [returnWire] }
def custodyReturned : Custody :=
  { usable := [returnedValue], preparing := [], acceptedFutures := [receivedValue, initialValue], inFlight := [] }
def custodyReturnRetired : Custody :=
  { usable := [returnedValue], preparing := [], acceptedFutures := [initialValue], inFlight := [] }
def custodyRetired : Custody :=
  { usable := [returnedValue], preparing := [], acceptedFutures := [], inFlight := [] }

theorem custodyInitialValueMatchesPendingHeader :
    (Hibana.observedActorView? ownerPending 10).map
      (fun view => (ownerPending.actor, view.role, view.run, view.nonce)) = some initialValue := by decide
theorem custodyReceivedValueMatchesSourceCommit :
    (custodyLoanRecv.ownedFacts? workerImage custodyChoreo).map
      (fun facts => (custodyLoanRecv.after.actor, facts.afterView.role,
        facts.afterView.run, facts.afterView.nonce)) = some receivedValue := by decide
theorem custodyReturnedValueMatchesSourceCommit :
    (custodyReturnRecv.ownedFacts? ownerImage custodyChoreo).map
      (fun facts => (custodyReturnRecv.after.actor, facts.afterView.role,
        facts.afterView.run, facts.afterView.nonce)) = some returnedValue := by decide
theorem custodyOriginalTagRetired :
    (Hibana.observedActorView? ownerSent 10).map
      (fun view => (ownerSent.actor, view.role, view.run, view.nonce)) ≠ some initialValue := by decide
theorem custodyReceiverTagRetired :
    (Hibana.observedActorView? workerSent 10).map
      (fun view => (workerSent.actor, view.role, view.run, view.nonce)) ≠ some receivedValue := by decide
theorem custodyReturnedTagFresh : returnedValue ≠ initialValue := by decide

theorem custodyUnpolledRuntimeUnchanged : ownerSealed = ownerBefore := by decide
theorem custodyPendingHeadersUnchanged : ownerPending.viewHeaders = ownerBefore.viewHeaders := by decide
theorem custodyPendingProgressUnchanged : ownerPending.progress = ownerBefore.progress := by decide
theorem custodyPendingPayloadBorrowed :
    (Hibana.observedActorView? ownerPending 10).map (·.operation) = some .sendPayload := by decide
theorem custodyReturnUnpolledRuntimeUnchanged : workerSealed = workerReceived := by decide
theorem custodyReturnFutureDropUnchanged : workerRetired = workerSent := by decide
theorem custodyOldFutureDropUnchanged : ownerRetired = ownerReturned := by decide

theorem custodySealStep : Step .seal custodyInitial custodySealed :=
  Step.seal custodyInitial [] [] initialValue
theorem custodyPendingStep : Step .unchanged custodySealed custodyPending :=
  Step.unchanged custodySealed
theorem custodyAcceptStep : Step .accept custodyPending custodySent :=
  Step.accept custodyPending [] [] initialValue loanWire
theorem custodyReceiveStep : Step .receive custodySent custodyReceived :=
  Step.receive custodySent [] loanWire receivedValue
theorem custodyReturnSealStep : Step .seal custodyReceived custodyReturnSealed :=
  Step.seal custodyReceived [] [] receivedValue
theorem custodyReturnAcceptStep : Step .accept custodyReturnSealed custodyReturnSent :=
  Step.accept custodyReturnSealed [] [] receivedValue returnWire
theorem custodyReturnReceiveStep : Step .receive custodyReturnSent custodyReturned :=
  Step.receive custodyReturnSent [] returnWire returnedValue
theorem custodyReturnRetireStep : Step .retireAcceptedFuture custodyReturned custodyReturnRetired :=
  Step.retireAcceptedFuture custodyReturned [] [initialValue] receivedValue
theorem custodyOldRetireStep : Step .retireAcceptedFuture custodyReturnRetired custodyRetired :=
  Step.retireAcceptedFuture custodyReturnRetired [] [] initialValue

theorem custodySealedExecution : Execution custodyInitial custodySealed :=
  .tail (.refl _) custodySealStep
theorem custodyPendingExecution : Execution custodyInitial custodyPending :=
  .tail custodySealedExecution custodyPendingStep
theorem custodySentExecution : Execution custodyInitial custodySent :=
  .tail custodyPendingExecution custodyAcceptStep
theorem custodyReceivedExecution : Execution custodyInitial custodyReceived :=
  .tail custodySentExecution custodyReceiveStep
theorem custodyReturnSealedExecution : Execution custodyInitial custodyReturnSealed :=
  .tail custodyReceivedExecution custodyReturnSealStep
theorem custodyReturnSentExecution : Execution custodyInitial custodyReturnSent :=
  .tail custodyReturnSealedExecution custodyReturnAcceptStep
theorem custodyReturnedExecution : Execution custodyInitial custodyReturned :=
  .tail custodyReturnSentExecution custodyReturnReceiveStep
theorem custodyReturnRetiredExecution : Execution custodyInitial custodyReturnRetired :=
  .tail custodyReturnedExecution custodyReturnRetireStep
theorem custodyRetiredExecution : Execution custodyInitial custodyRetired :=
  .tail custodyReturnRetiredExecution custodyOldRetireStep

theorem custodyUnpolledValueIsNotApplicationAuthority :
    custodySealed.usable = [] ∧ custodySealed.preparing = [initialValue] := by decide
theorem custodyTwoPrivateFuturesAreNotTwoAuthorities :
    custodyReturned.acceptedFutures.length = 2 ∧ custodyReturned.authorityCount ≤ 1 := by
  exact ⟨by decide, one_binding_has_at_most_one_transferable_authority custodyReturnedExecution⟩
theorem custodyFinalValueIsUnique : custodyRetired.usable.length ≤ 1 :=
  one_binding_cannot_have_two_application_values custodyRetiredExecution
theorem custodyCannotTreatTheUnpolledPayloadAsAnApplicationValue :
    ¬ Execution custodyInitial ({ custodySealed with usable := [initialValue] } : Custody) := by
  intro execution
  have bound := one_binding_has_at_most_one_transferable_authority execution
  simp [Observation.authorityCount, custodySealed] at bound
"#);
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("FutureCustodyGenerated.lean"), source).unwrap();
}
