mod common;
#[path = "support/cursor_send_recv.rs"]
mod cursor_harness;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::task::{Context, Poll};

use common::TestTx;
use cursor_harness::*;
use frame_payload::FramePayload;
use hibana::runtime::{
    transport::{ReceivedFrame, Transport, TransportError},
    wire::Payload,
};

#[path = "cursor_send_recv/direct_recv.rs"]
mod direct_recv;
