//! The production send loop must retain its input before admission, without
//! reaching I/O or publishing a commit. Descriptor admission is checked apart.
use super::*;

struct WaitingEndpoint;

impl<'r> SendKernelEndpoint<'r> for WaitingEndpoint {
    fn poll_send_preview_kernel(
        &mut self,
        _label: u8,
        _kind: crate::eff::EventPayload,
    ) -> Poll<SendResult<SendPreview>> {
        Poll::Pending
    }

    fn poll_send_init_kernel(
        &mut self,
        _descriptor: SendRuntimeDesc,
        _meta: SendMeta,
        _index: Option<StateIndex>,
        _authority: SendRouteAuthority,
        _payload: Option<lane_port::SendInput>,
    ) -> SendInitOutcome<'r> {
        panic!("a waiting preview cannot initialize transport")
    }

    fn poll_send_pending_kernel(
        &mut self,
        _pending: &mut PendingSendIo<'r>,
        _cx: &mut core::task::Context<'_>,
    ) -> Poll<SendResult<SendCommitPlan<'r>>> {
        panic!("a waiting preview cannot poll transport")
    }

    fn finish_send_after_transport_kernel(
        &mut self,
        _plan: SendCommitPlan<'r>,
    ) -> SendCommitOutcome<'r> {
        panic!("a waiting preview cannot commit")
    }
}

#[kani::proof]
#[kani::unwind(3)]
fn a_pending_preview_retains_the_input_without_io_or_commit() {
    let label: u8 = kani::any();
    let schema: u32 = kani::any();
    let value: u32 = kani::any();
    let kind = crate::eff::EventPayload::Wire(schema);
    let mut state = SendState::Preparing {
        logical_label: label,
        payload_kind: kind,
        input: None,
    };
    let mut input = Some(lane_port::SendInput::Payload(
        lane_port::RawSendPayload::from_typed(&value),
    ));
    let mut endpoint = WaitingEndpoint;
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    for _ in 0..2 {
        assert!(kernel_send(&mut endpoint, &mut state, &mut input, &mut cx).is_pending());
        assert!(input.is_none());
        match &state {
            SendState::Preparing {
                logical_label,
                payload_kind,
                input,
            } => {
                assert_eq!(*logical_label, label);
                assert_eq!(*payload_kind, kind);
                assert!(matches!(input, Some(lane_port::SendInput::Payload(_))));
            }
            SendState::Sending { .. } | SendState::Done => panic!("waiting lost its input"),
        }
    }
}
