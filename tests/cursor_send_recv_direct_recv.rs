mod common;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use core::task::{Context, Poll};

use common::TestTransport;
use common::TestTx;
use frame_payload::FramePayload;
use hibana::runtime::{
    transport::{ReceivedFrame, Transport, TransportError},
    wire::Payload,
};
use hibana::{
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
    },
};

#[path = "cursor_send_recv/direct_recv.rs"]
mod direct_recv;
#[path = "cursor_send_recv/recv_lane_census.rs"]
mod recv_lane_census;
