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
    future::Future,
    task::{Context, Poll},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

use cursor_harness::*;
use frame_payload::FramePayload;
use hibana::runtime::transport::{ReceivedFrame, Transport};

#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
