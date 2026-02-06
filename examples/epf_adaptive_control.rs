//! EPF VM adaptive control demonstration.
//!
//! This example demonstrates how EPF bytecode can autonomously make control decisions
//! based on runtime metrics. The policy monitors retry counts and issues a CHECKPOINT
//! when the count exceeds a threshold.
//!
//! Key concepts:
//! - `JUMP_GT` instruction for threshold comparisons
//! - `GET_RETRY` to read transport metrics
//! - `ACT_EFFECT Checkpoint` to trigger control-plane actions
//! - `ACT_ABORT` for policy-driven abort
//!
//! Run with:
//! ```bash
//! cargo run --example epf_adaptive_control --features std
//! ```

#![cfg(feature = "std")]

use hibana::{
    control::{CpEffect, cap::CapsMask},
    epf::{
        dispatch::ensure_allowed,
        ops,
        vm::{Slot, Vm, VmAction, VmCtx},
    },
    observe::TapEvent,
    transport::TransportSnapshot,
};

// ============================================================================
// EPF Bytecode Programs
// ============================================================================

/// Adaptive checkpoint policy: issue CHECKPOINT when retry count > threshold.
///
/// Assembly:
/// ```text
///     LOAD_IMM r1, 5              ; threshold = 5
///     GET_RETRY r0                ; r0 = retry count
///     JUMP_GT r0, r1, checkpoint  ; if r0 > r1 goto checkpoint
///     HALT                        ; normal exit
/// checkpoint:
///     ACT_EFFECT Checkpoint, r0   ; issue checkpoint
///     HALT
/// ```
const CHECKPOINT_ON_RETRY_POLICY: &[u8] = &[
    // 0x00: LOAD_IMM r1, 5 (6 bytes)
    ops::instr::LOAD_IMM, 0x01, 0x05, 0x00, 0x00, 0x00,
    // 0x06: GET_RETRY r0 (2 bytes)
    ops::instr::GET_RETRY, 0x00,
    // 0x08: JUMP_GT r0, r1, 0x0E (checkpoint label) (5 bytes)
    ops::instr::JUMP_GT, 0x00, 0x01, 0x0E, 0x00,
    // 0x0D: HALT (normal path) (1 byte)
    ops::instr::HALT,
    // 0x0E: checkpoint - ACT_EFFECT Checkpoint (0x03), r0 (3 bytes)
    ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00,
    // 0x11: HALT (1 byte)
    ops::instr::HALT,
];

/// Aggressive abort policy: issue ABORT when retry count > threshold.
///
/// Assembly:
/// ```text
///     LOAD_IMM r1, 3              ; threshold = 3
///     GET_RETRY r0                ; r0 = retry count
///     JUMP_GT r0, r1, abort       ; if r0 > r1 goto abort
///     HALT                        ; normal exit
/// abort:
///     ACT_ABORT 0x0001            ; abort with reason 1
/// ```
const ABORT_ON_RETRY_POLICY: &[u8] = &[
    // 0x00: LOAD_IMM r1, 3 (6 bytes)
    ops::instr::LOAD_IMM, 0x01, 0x03, 0x00, 0x00, 0x00,
    // 0x06: GET_RETRY r0 (2 bytes)
    ops::instr::GET_RETRY, 0x00,
    // 0x08: JUMP_GT r0, r1, 0x0E (abort label) (5 bytes)
    ops::instr::JUMP_GT, 0x00, 0x01, 0x0E, 0x00,
    // 0x0D: HALT (normal path) (1 byte)
    ops::instr::HALT,
    // 0x0E: abort - ACT_ABORT 0x0001 (3 bytes)
    ops::instr::ACT_ABORT, 0x01, 0x00,
];

/// Latency-aware checkpoint policy: checkpoint when latency > 100ms (100000µs).
///
/// Assembly:
/// ```text
///     LOAD_IMM r1, 100000         ; threshold = 100000µs
///     GET_LATENCY r0              ; r0 = latency
///     JUMP_GT r0, r1, checkpoint  ; if r0 > r1 goto checkpoint
///     HALT
/// checkpoint:
///     ACT_EFFECT Checkpoint, r0
///     HALT
/// ```
const LATENCY_CHECKPOINT_POLICY: &[u8] = &[
    // 0x00: LOAD_IMM r1, 100000 (0x000186A0) (6 bytes)
    ops::instr::LOAD_IMM, 0x01, 0xA0, 0x86, 0x01, 0x00,
    // 0x06: GET_LATENCY r0 (2 bytes)
    ops::instr::GET_LATENCY, 0x00,
    // 0x08: JUMP_GT r0, r1, 0x0E (checkpoint) (5 bytes)
    ops::instr::JUMP_GT, 0x00, 0x01, 0x0E, 0x00,
    // 0x0D: HALT (1 byte)
    ops::instr::HALT,
    // 0x0E: checkpoint (3 bytes)
    ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00,
    // 0x11: HALT (1 byte)
    ops::instr::HALT,
];

// ============================================================================
// Test Harness
// ============================================================================

fn run_policy_with_metrics(
    policy_name: &str,
    code: &[u8],
    retry_count: u32,
    latency_us: Option<u64>,
    caps: CapsMask,
) {
    println!("\n=== {} ===", policy_name);
    println!("  retry_count: {}", retry_count);
    println!("  latency_us: {:?}", latency_us);

    let mut scratch = [0u8; 64];
    let mut vm = Vm::new(code, &mut scratch, 100);

    // Create a dummy tap event for context
    let event = TapEvent::default();
    let mut ctx = VmCtx::new(Slot::Rendezvous, &event, caps);

    // Inject transport metrics
    ctx.set_transport_snapshot(TransportSnapshot {
        retransmissions: Some(retry_count),
        latency_us,
        ..Default::default()
    });

    let action = vm.execute(&mut ctx);

    match action {
        VmAction::Proceed => {
            println!("  result: Proceed (no action taken)");
        }
        VmAction::Abort { reason } => {
            println!("  result: Abort (reason=0x{:04X})", reason);
        }
        VmAction::Ra(ra_op) => {
            // Validate capability
            match ensure_allowed(Slot::Rendezvous, caps, ra_op) {
                Ok(op) => {
                    println!("  result: Control action {:?}", op);
                }
                Err(e) => {
                    println!("  result: Control action denied - {:?}", e);
                }
            }
        }
        VmAction::Trap(trap) => {
            println!("  result: TRAP {:?}", trap);
        }
        VmAction::Tap { id, arg0, arg1 } => {
            println!("  result: Tap (id=0x{:04X}, arg0={}, arg1={})", id, arg0, arg1);
        }
        VmAction::Route { arm } => {
            println!("  result: Route (arm={})", arm);
        }
    }
}

fn main() {
    println!("EPF VM Adaptive Control Demonstration");
    println!("======================================");

    // Capability set that allows checkpoint
    let checkpoint_caps = CapsMask::empty().with(CpEffect::Checkpoint);

    // Test 1: Checkpoint policy with low retry count (should NOT trigger)
    run_policy_with_metrics(
        "Checkpoint Policy (retry=2, threshold=5)",
        CHECKPOINT_ON_RETRY_POLICY,
        2,
        None,
        checkpoint_caps,
    );

    // Test 2: Checkpoint policy with high retry count (should trigger)
    run_policy_with_metrics(
        "Checkpoint Policy (retry=10, threshold=5)",
        CHECKPOINT_ON_RETRY_POLICY,
        10,
        None,
        checkpoint_caps,
    );

    // Test 3: Abort policy with low retry count (should NOT trigger)
    run_policy_with_metrics(
        "Abort Policy (retry=2, threshold=3)",
        ABORT_ON_RETRY_POLICY,
        2,
        None,
        CapsMask::empty(),
    );

    // Test 4: Abort policy with high retry count (should trigger)
    run_policy_with_metrics(
        "Abort Policy (retry=5, threshold=3)",
        ABORT_ON_RETRY_POLICY,
        5,
        None,
        CapsMask::empty(),
    );

    // Test 5: Latency checkpoint with low latency (should NOT trigger)
    run_policy_with_metrics(
        "Latency Checkpoint (50ms, threshold=100ms)",
        LATENCY_CHECKPOINT_POLICY,
        0,
        Some(50_000),
        checkpoint_caps,
    );

    // Test 6: Latency checkpoint with high latency (should trigger)
    run_policy_with_metrics(
        "Latency Checkpoint (200ms, threshold=100ms)",
        LATENCY_CHECKPOINT_POLICY,
        0,
        Some(200_000),
        checkpoint_caps,
    );

    // Test 7: Checkpoint denied due to missing capability
    run_policy_with_metrics(
        "Checkpoint Policy (retry=10, NO CAPS)",
        CHECKPOINT_ON_RETRY_POLICY,
        10,
        None,
        CapsMask::empty(), // No checkpoint capability
    );

    println!("\n======================================");
    println!("Demonstration complete.");
}
