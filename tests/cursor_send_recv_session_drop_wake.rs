mod common;
#[path = "support/cursor_send_recv.rs"]
mod cursor_harness;
#[path = "support/frame_payload.rs"]
mod frame_payload;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use core::{
    cell::Cell,
    future::Future,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use cursor_harness::*;
use frame_payload::FramePayload;

#[path = "cursor_send_recv/session_drop_wake.rs"]
mod session_drop_wake;
