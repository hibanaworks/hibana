mod common;
#[path = "support/cursor_send_recv.rs"]
mod cursor_harness;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/tls_ref.rs"]
mod tls_ref_support;

use cursor_harness::*;

#[path = "cursor_send_recv/send_recv.rs"]
mod send_recv;
