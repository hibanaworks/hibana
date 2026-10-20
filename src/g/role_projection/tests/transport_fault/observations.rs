//! Serialize copied observations after the actual Endpoint execution.
use super::*;
use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
use core::{fmt::Write, task::Poll};
use std::format;

pub(super) fn write(
    [owner_image, worker_image]: [&'static RoleImageRef; 2],
    direction: &str,
    samples: &[Sample],
) {
    let mut source = format!(
        "import Hibana.SourceOccurrenceTrace\n\nopen Hibana Hibana.EndpointDelegation Hibana.OwnedCallHistory\n\n\
         def faultChoreo : Choreo := {}\n\
         def ownerImage : RustDescriptorImage := {}\n\
         def workerImage : RustDescriptorImage := {}\n\
         def faultImages (actor : Nat) : RustDescriptorImage := if actor = 1 then workerImage else ownerImage\n\n",
        Steps::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(owner_image)),
        descriptor_source(RoleDescriptorRef::from_resident(worker_image)),
    );
    for (index, sample) in samples.iter().enumerate() {
        writeln!(source, "namespace Case{index}\n").unwrap();
        for (name, cut) in &sample.cuts {
            writeln!(
                source,
                "def {name} : ActorProgressObservation := {}\n\
                def {name}Mutations : Nat := {}\n",
                super::super::commit::observation_source(cut),
                cut.completion_mutations
            )
            .unwrap();
        }
        for (name, frame) in &sample.frames {
            let header = frame.header;
            writeln!(source, "def {name}Wire : OwnedWireObservation := {{ session := {}, lane := {}, sender := {}, receiver := {}, frameLabel := {}, payload := {:?} }}\n\
                def {name}Witness : Nat := {}\n",
                SID.raw(), header.lane(), header.source_role(), header.target_role(),
                header.label().raw(), frame.payload(), frame.witness).unwrap();
        }
        let (io, before, after) = match sample.fault.at {
            At::SendAfterAcceptance => ("send", "ownerBefore", "ownerAfter"),
            At::ReceiveBeforeDelivery => ("receive", "workerBefore", "workerAfter"),
        };
        let error = match sample.fault.error {
            TransportError::Offline => "offline",
            TransportError::Deadline => "deadline",
            TransportError::Capacity => "capacity",
            TransportError::Failed => "failed",
        };
        writeln!(source, "def actualFault : ActorTransportFaultObservation := {{ session := {}, view := {}, lane := {}, io := .{io}, error := .{error}, before := {before}, after := {after} }}\n",
            sample.fault.session.raw(), sample.fault.view, sample.fault.lane).unwrap();
        let expected = match sample.fault.at {
            At::SendAfterAcceptance => matches!(
                sample.captured.as_slice(),
                [
                    OwnedFutureBoundary::Seal { .. },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Ready(Err(_)),
                        ..
                    }
                ]
            ),
            At::ReceiveBeforeDelivery => matches!(
                sample.captured.as_slice(),
                [
                    OwnedFutureBoundary::Seal { .. },
                    OwnedFutureBoundary::Poll {
                        result: Poll::Ready(Ok(())),
                        ..
                    }
                ]
            ),
        };
        assert!(
            expected,
            "the actual constructor and completed poll, without an invented drop"
        );
        let OwnedFutureBoundary::Seal {
            carrier,
            payload,
            label,
            region,
        } = sample.captured[0]
        else {
            unreachable!()
        };
        assert_eq!(carrier.actor_address, payload.actor_address);
        let before = &sample
            .cuts
            .iter()
            .find(|(name, _)| *name == "ownerBefore")
            .unwrap()
            .1;
        for identity in [carrier, payload] {
            let view = before
                .views
                .iter()
                .find(|view| view.role == identity.role)
                .unwrap();
            assert_eq!((identity.run, identity.nonce), (view.run, view.nonce));
        }
        writeln!(source, "def loanRequest : Request := {{ carrier := (0, {}, {}, {}), payload := (0, {}, {}, {}), label := {label}, borrow := ({}, {}) }}\n",
            carrier.role, carrier.run, carrier.nonce, payload.role, payload.run, payload.nonce,
            region.bounds().start, region.bounds().end).unwrap();
        let commits: &[(&str, &str, &str, u8, &str)] = match sample.fault.at {
            At::SendAfterAcceptance => &[
                (
                    "loanRecv",
                    "workerBefore",
                    "workerReceived",
                    1,
                    "loanReceivedWire",
                ),
                ("readSend", "workerReceived", "workerSent", 10, "readWire"),
                (
                    "readRecv",
                    "workerSent",
                    "workerDone",
                    1,
                    "readReceivedWire",
                ),
            ],
            At::ReceiveBeforeDelivery => {
                &[("loanSend", "ownerBefore", "ownerAfter", 0, "loanWire")]
            }
        };
        for &(name, before, after, view, wire) in commits {
            let before_cut = &sample
                .cuts
                .iter()
                .find(|(name, _)| *name == before)
                .unwrap()
                .1;
            let after_cut = &sample
                .cuts
                .iter()
                .find(|(name, _)| *name == after)
                .unwrap()
                .1;
            let changed: Vec<_> = before_cut
                .steps
                .iter()
                .zip(&after_cut.steps)
                .enumerate()
                .filter_map(|(index, (old, new))| (old.done != new.done).then_some(index))
                .collect();
            assert_eq!(changed.len(), 1);
            writeln!(source, "def {name} : ActorCommitRecord := {{ wire := {wire}, step := {{ pathIndex := 0, event := {}, view := {view}, before := {before}, after := {after} }} }}\n",
                changed[0]).unwrap();
        }
        writeln!(source, "\nend Case{index}\n").unwrap();
    }
    // The full source/custody word uses the actual Deadline case. Keep its
    // public claims at the root so the existing kernel inventory names them.
    source.push_str("\nopen Case1\n\n");
    source.push_str(match samples[1].fault.at {
        At::SendAfterAcceptance => include_str!("send.lean"),
        At::ReceiveBeforeDelivery => include_str!("receive.lean"),
    });
    writeln!(source, "def actualFaults : List ActorTransportFaultObservation := [{}]\n\
        theorem all_actual_transport_errors_match_their_bound_ports :\n\
        actualFaults.all (fun sample => sample.check (faultImages sample.before.actor) faultChoreo {}) = true := by decide\n",
        (0..samples.len()).map(|index| format!("Case{index}.actualFault")).collect::<Vec<_>>().join(", "), SID.raw()).unwrap();
    source.push_str(
        "\ntheorem all_actual_faults_reject_rebinding_missing_fault_and_hidden_progress :\n\
         actualFaults.all (fun sample =>\n\
           !({ sample with session := 0 }).check (faultImages sample.before.actor) faultChoreo 9077 &&\n\
           !({ sample with view := 255 }).check (faultImages sample.before.actor) faultChoreo 9077 &&\n\
           !({ sample with after := { sample.after with fault := none } }).check (faultImages sample.before.actor) faultChoreo 9077 &&\n\
           !({ sample with after := { sample.after with steps := sample.after.steps.map (fun (step : ActorStepObservation) => { step with done := true }) } }).check (faultImages sample.before.actor) faultChoreo 9077) = true := by decide\n");
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("Transport{direction}FaultGenerated.lean")),
        source,
    )
    .unwrap();
}
