mod common;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::cell::UnsafeCell;

use common::TestTransport;
use hibana::{
    g::{self, Msg},
    runtime::program::{RoleProgram, project},
    runtime::{
        Config, SessionKit, SessionKitStorage,
        ids::SessionId,
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};
use runtime_support::with_runtime_workspace;
use tls_ref_support::with_resident_tls_ref;

#[derive(Clone, Copy)]
struct InstallPayload {
    data: [u8; 4],
}

impl WireEncode for InstallPayload {
    fn encoded_len(&self) -> Option<usize> {
        Some(4)
    }

    fn encode_into(&self, buf: &mut [u8]) -> Result<usize, CodecError> {
        if buf.len() < 4 {
            return Err(CodecError::Truncated);
        }
        buf[..4].copy_from_slice(&self.data);
        Ok(4)
    }
}

impl WirePayload for InstallPayload {
    type Decoded<'a> = Self;

    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if input.as_bytes().len() == 4 {
            Ok(())
        } else if input.as_bytes().len() < 4 {
            Err(CodecError::Truncated)
        } else {
            Err(CodecError::Malformed)
        }
    }

    fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
        let input = input.as_bytes();
        let mut data = [0u8; 4];
        data.copy_from_slice(&input[..4]);
        Self { data }
    }

    fn zero_payload<'a>(scratch: &'a mut [u8]) -> Result<Payload<'a>, CodecError> {
        if scratch.len() < 4 {
            return Err(CodecError::Truncated);
        }
        scratch[..4].fill(0);
        Ok(Payload::new(&scratch[..4]))
    }
}

type TestKit = SessionKit<'static, TestTransport, 2>;
type TestKitStorage = SessionKitStorage<'static, TestTransport, 2>;

std::thread_local! {
    static SESSION_SLOT: UnsafeCell<TestKitStorage> = const {
        UnsafeCell::new(SessionKitStorage::uninit())
    };
}

fn transport_queue_is_empty(transport: &TestTransport) -> bool {
    transport.queue_is_empty()
}

fn run_local_action_flow(
    cluster: &'static TestKit,
    slab: &'static mut [u8],
    transport: &TestTransport,
) {
    let program = g::send::<0, 0, Msg<7, InstallPayload>>();
    let actor_program: RoleProgram<0> = project(&program);
    let rv = cluster
        .rendezvous(Config::from_resources(slab), transport.clone())
        .expect("register rendezvous");

    let sid = SessionId::new(42);

    let payload = InstallPayload {
        data: [0x13, 0x37, 0xC0, 0xDE],
    };

    let mut endpoint = rv
        .session(sid)
        .role(&actor_program)
        .enter()
        .expect("attach actor endpoint");
    let () = futures::executor::block_on(
        endpoint
            .flow::<Msg<7, InstallPayload>>()
            .expect("install flow")
            .send(&payload),
    )
    .expect("local action succeeded");
    assert!(transport_queue_is_empty(transport));
}

#[test]
fn local_action_flow_executes() {
    with_runtime_workspace(|slab| {
        let transport = TestTransport::new();
        with_resident_tls_ref(&SESSION_SLOT, |cluster| {
            run_local_action_flow(cluster, slab, &transport);
        });
    });
}
