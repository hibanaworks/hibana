//! Exercise the actual non-generic send loop at its prepared-I/O boundary.
//! The endpoint double supplies each poll result and records publication.
//! It does not validate source admission, carrier delivery or the commit body.
use super::*;
use crate::endpoint::kernel::core::{
    PendingSendIo, SendCommitOutcome, SendCommitPlan, SendCommitProof, SendInitOutcome,
    SendKernelEndpoint, SendPreview, SendProgressCommitPlan, SendRouteAudit, SendRouteAuthority,
    SendRuntimeDesc, SendState, kernel_send,
};
use crate::endpoint::kernel::lane_port;
use crate::endpoint::{SendError, SendResult};
use core::task::{Context, Poll, Waker};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultClass {
    Pending,
    Success,
    Failure,
}

struct CompletionEndpoint {
    outcome: ResultClass,
    polls: u8,
    publications: u8,
}

impl<'r> SendKernelEndpoint<'r> for CompletionEndpoint {
    fn poll_send_preview_kernel(
        &mut self,
        _: u8,
        _: crate::eff::EventPayload,
    ) -> Poll<SendResult<SendPreview>> {
        panic!("a prepared send must not preview again")
    }

    fn poll_send_init_kernel(
        &mut self,
        _: SendRuntimeDesc,
        _: crate::global::typestate::SendMeta,
        _: Option<StateIndex>,
        _: SendRouteAuthority,
        _: Option<lane_port::SendInput>,
    ) -> SendInitOutcome<'r> {
        panic!("a prepared send must not initialize again")
    }

    fn poll_send_pending_kernel(
        &mut self,
        pending: &mut PendingSendIo<'r>,
        _: &mut Context<'_>,
    ) -> Poll<SendResult<SendCommitPlan<'r>>> {
        self.polls += 1;
        match self.outcome {
            ResultClass::Pending => Poll::Pending,
            ResultClass::Success => Poll::Ready(Ok(pending.commit_plan.take().unwrap())),
            ResultClass::Failure => {
                pending.commit_plan = None;
                Poll::Ready(Err(SendError::PhaseInvariant))
            }
        }
    }

    fn finish_send_after_transport_kernel(
        &mut self,
        _: SendCommitPlan<'r>,
    ) -> SendCommitOutcome<'r> {
        self.publications += 1;
        SendCommitOutcome {
            _borrow: core::marker::PhantomData,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Observation {
    boundary: ResultClass,
    returned: ResultClass,
    polls: u8,
    publications: u8,
    done: bool,
    retained: bool,
}

fn observe(boundary: ResultClass) -> Observation {
    // The real loop treats this private plan as opaque. No mock plan is ever
    // passed to a real OperationKernel or used to authorize a source step.
    let plan = SendCommitPlan {
        proof: SendCommitProof {
            transfer: None,
            progress: SendProgressCommitPlan {
                delta: PreparedCommitDelta::from_preflighted(
                    CommitDelta::cursor_only(StateIndex::new(0)),
                    PreparedRouteCommitRows::empty(),
                    None,
                    None,
                ),
                route_audit: SendRouteAudit::None,
            },
            _borrow: core::marker::PhantomData,
        },
    };
    let mut state = SendState::Sending {
        pending: PendingSendIo {
            lane: crate::session::types::Lane::new(0),
            transport: lane_port::PendingSend::new(
                lane_port::SendInput::Payload(lane_port::RawSendPayload::from_typed(&())),
                crate::transport::SendMeta {
                    eff_index: crate::eff::EffIndex::ZERO,
                    logical_label: crate::transport::LogicalLabel::new(0),
                    frame_label: crate::transport::FrameLabel::new(0),
                    target_role: 1,
                    lane: 0,
                },
                lane_port::SendCarrier::Transport,
            ),
            commit_plan: Some(plan),
        },
    };
    let mut endpoint = CompletionEndpoint {
        outcome: boundary,
        polls: 0,
        publications: 0,
    };
    let result = kernel_send(
        &mut endpoint,
        &mut state,
        &mut None,
        &mut Context::from_waker(Waker::noop()),
    );
    let returned = match result {
        Poll::Pending => ResultClass::Pending,
        Poll::Ready(Ok(_)) => ResultClass::Success,
        Poll::Ready(Err(_)) => ResultClass::Failure,
    };
    let (done, retained) = match state {
        SendState::Sending { pending } => (false, pending.commit_plan.is_some()),
        SendState::Done => (true, false),
        SendState::Preparing { .. } => panic!("a prepared send cannot go backwards"),
    };
    Observation {
        boundary,
        returned,
        polls: endpoint.polls,
        publications: endpoint.publications,
        done,
        retained,
    }
}

fn check(observed: Observation) {
    assert_eq!(observed.returned, observed.boundary);
    assert_eq!(observed.polls, 1);
    match observed.boundary {
        ResultClass::Pending => {
            assert_eq!(observed.publications, 0);
            assert!(!observed.done);
            assert!(observed.retained);
        }
        ResultClass::Success => {
            assert_eq!(observed.publications, 1);
            assert!(observed.done);
            assert!(!observed.retained);
        }
        ResultClass::Failure => {
            assert_eq!(observed.publications, 0);
            assert!(observed.done);
            assert!(!observed.retained);
        }
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(2)]
fn a_prepared_send_publishes_or_finishes_without_post_acceptance_pending() {
    let boundary = match kani::any::<u8>() % 3 {
        0 => ResultClass::Pending,
        1 => ResultClass::Success,
        _ => ResultClass::Failure,
    };
    check(observe(boundary));
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn a_prepared_send_publishes_before_returning_success() {
    let observations = [
        ResultClass::Pending,
        ResultClass::Success,
        ResultClass::Failure,
    ]
    .map(observe);
    for observed in observations {
        check(observed);
    }
    #[cfg(not(miri))]
    {
        let class = |value| match value {
            ResultClass::Pending => ".pending",
            ResultClass::Success => ".success",
            ResultClass::Failure => ".failure",
        };
        let mut source = std::string::String::from(
            "import Hibana.SendCompletion\n\ndef observed : List Hibana.SendCompletionObservation := [\n",
        );
        for (index, sample) in observations.iter().enumerate() {
            if index != 0 {
                source.push_str(",\n");
            }
            source.push_str(&std::format!(
                "  {{ boundary := {}, returned := {}, polls := {}, publications := {}, done := {}, retained := {} }}",
                class(sample.boundary), class(sample.returned), sample.polls,
                sample.publications, sample.done, sample.retained,
            ));
        }
        source.push_str("\n]\n");
        source.push_str(r#"
def successful : Hibana.SendCompletionObservation := observed[1]'(by decide)

theorem actualKernelCompletionTableIsExact :
    Hibana.checkSendCompletionTable observed = true := by decide

theorem actualSuccessPublishesBeforeReturn :
    successful.returned = .success ∧ successful.publications = 1 ∧
      successful.done = true ∧ successful.retained = false := by decide

theorem aSuccessThatParksInsteadIsRejected :
    ({ successful with returned := .pending } : Hibana.SendCompletionObservation).check = false := by decide

theorem aSuccessWithoutPublicationIsRejected :
    ({ successful with publications := 0 } : Hibana.SendCompletionObservation).check = false := by decide

theorem aMissingBoundaryCaseIsRejected :
    Hibana.checkSendCompletionTable observed.tail = false := by decide

theorem actualTableCoversEveryAbstractBoundary (boundary : Hibana.SendPollClass) :
    ∃ sample, sample ∈ observed ∧ sample.boundary = boundary ∧ sample.check = true :=
  Hibana.a_checked_send_completion_table_covers_every_boundary actualKernelCompletionTableIsExact boundary

theorem actualKernelSuccessPublishesItsCheckedOwnedSourceTag
    {sample : Hibana.ActorCommitObservation} {image : Hibana.RustDescriptorImage}
    {choreo : Hibana.Choreo} {wire : Hibana.OwnedWireObservation} {session : Nat}
    {facts : Hibana.OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (sending : facts.side = .send) :
    successful.returned = .success ∧ successful.publications = 1 ∧ successful.done = true ∧
      Hibana.observedResourceTag? sample.after facts.resource.role = none :=
  Hibana.checked_owned_send_publication_finishes_without_another_wait
    (by decide) (by decide) checked decoded sending
"#);
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("SendCompletionGenerated.lean"), source).unwrap();
    }
}
