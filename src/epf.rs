//! EPF — Effect Policy Filter (no_std / no_alloc).
//!
//! This layer brings together the EPF bytecode loader, verifier, dispatcher,
//! and interpreter to evaluate the `GenericCapToken<K> → CapsMask → CpEffect` pipeline.
//! The VM uses eight 32-bit registers and a fixed-size memory region, keeping
//! the design small enough to run on the rendezvous hot path without `std` or
//! heap allocations. The host side maps the emitted [`Action`] values back into
//! the control and data planes.

/// EPF dispatch glue for rendezvous integration.
pub(crate) mod dispatch;
/// EPF host interface.
pub(crate) mod host;
/// Bytecode image loader.
#[cfg(test)]
pub(crate) mod loader;
/// Opcode definitions.
pub(crate) mod ops;
/// Slot-level policy contract (SNC).
pub(crate) mod slot_contract;
/// Bytecode verifier.
pub(crate) mod verifier;
/// VM execution engine.
pub(crate) mod vm;
use crate::observe::core::TapEvent;
use host::HostSlots;
use vm::VmCtx;
use vm::{Slot, Trap, VmAction};

use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, SessionId},
    transport::TransportSnapshot,
};

/// Abort outcome emitted by the policy VM (or by the host when mapping traps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbortInfo {
    pub reason: u16,
    pub trap: Option<Trap>,
}

/// Engine-level fail-closed reason used when EPF execution cannot produce
/// a safe decision (trap / verifier failure / illegal syscall path).
pub(crate) const ENGINE_FAIL_CLOSED: u16 = 0xFFFF;
/// Engine-level liveness exhaustion reason for dynamic route decision loops.
pub(crate) const ENGINE_LIVENESS_EXHAUSTED: u16 = 0xFFFE;

/// Runtime policy mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyMode {
    Shadow,
    Enforce,
}

/// Reduced emergency-plane verdict domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PolicyVerdict {
    Proceed,
    RouteArm(u8),
    Reject(u16),
}

#[inline]
pub(crate) const fn policy_mode_tag(mode: PolicyMode) -> u8 {
    match mode {
        PolicyMode::Shadow => 0,
        PolicyMode::Enforce => 1,
    }
}

#[inline]
pub(crate) const fn verdict_tag(verdict: PolicyVerdict) -> u8 {
    match verdict {
        PolicyVerdict::Proceed => 0,
        PolicyVerdict::RouteArm(_) => 1,
        PolicyVerdict::Reject(_) => 2,
    }
}

#[inline]
pub(crate) const fn verdict_arm(verdict: PolicyVerdict) -> u8 {
    match verdict {
        PolicyVerdict::RouteArm(arm) => arm,
        _ => 0,
    }
}

#[inline]
pub(crate) const fn verdict_reason(verdict: PolicyVerdict) -> u16 {
    match verdict {
        PolicyVerdict::Reject(reason) => reason,
        _ => 0,
    }
}

#[inline]
pub(crate) const fn slot_tag(slot: Slot) -> u8 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

const FNV32_OFFSET: u32 = 0x811C_9DC5;
const FNV32_PRIME: u32 = 0x0100_0193;

#[inline]
fn fnv32_mix_u8(mut hash: u32, byte: u8) -> u32 {
    hash ^= byte as u32;
    hash.wrapping_mul(FNV32_PRIME)
}

#[inline]
fn fnv32_mix_u16(hash: u32, value: u16) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    fnv32_mix_u8(hash, bytes[1])
}

#[inline]
fn fnv32_mix_u32(hash: u32, value: u32) -> u32 {
    let bytes = value.to_le_bytes();
    let hash = fnv32_mix_u8(hash, bytes[0]);
    let hash = fnv32_mix_u8(hash, bytes[1]);
    let hash = fnv32_mix_u8(hash, bytes[2]);
    fnv32_mix_u8(hash, bytes[3])
}

#[inline]
fn fnv32_mix_u64(hash: u32, value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    let mut out = hash;
    let mut idx = 0usize;
    while idx < bytes.len() {
        out = fnv32_mix_u8(out, bytes[idx]);
        idx += 1;
    }
    out
}

#[inline]
fn fnv32_mix_opt_u32(hash: u32, value: Option<u32>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u32(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

#[inline]
fn fnv32_mix_opt_u64(hash: u32, value: Option<u64>) -> u32 {
    match value {
        Some(v) => fnv32_mix_u64(fnv32_mix_u8(hash, 1), v),
        None => fnv32_mix_u8(hash, 0),
    }
}

/// Deterministic 32-bit hash of tap input consumed by EPF.
#[inline]
pub(crate) fn hash_tap_event(event: &TapEvent) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_u32(hash, event.ts);
    hash = fnv32_mix_u16(hash, event.id);
    hash = fnv32_mix_u16(hash, event.causal_key);
    hash = fnv32_mix_u32(hash, event.arg0);
    hash = fnv32_mix_u32(hash, event.arg1);
    fnv32_mix_u32(hash, event.arg2)
}

/// Deterministic 32-bit hash of EPF input args.
#[inline]
pub(crate) fn hash_policy_input(input: [u32; 4]) -> u32 {
    let mut hash = FNV32_OFFSET;
    let mut idx = 0usize;
    while idx < input.len() {
        hash = fnv32_mix_u32(hash, input[idx]);
        idx += 1;
    }
    hash
}

/// Deterministic 32-bit hash of transport snapshot attached to VM context.
#[inline]
pub(crate) fn hash_transport_snapshot(snapshot: TransportSnapshot) -> u32 {
    let mut hash = FNV32_OFFSET;
    hash = fnv32_mix_opt_u64(hash, snapshot.latency_us);
    hash = fnv32_mix_opt_u32(hash, snapshot.queue_depth);
    hash = fnv32_mix_opt_u64(hash, snapshot.pacing_interval_us);
    hash = fnv32_mix_opt_u32(hash, snapshot.congestion_marks);
    hash = fnv32_mix_opt_u32(hash, snapshot.retransmissions);
    hash = fnv32_mix_opt_u32(hash, snapshot.pto_count);
    hash = fnv32_mix_opt_u64(hash, snapshot.srtt_us);
    hash = fnv32_mix_opt_u64(hash, snapshot.latest_ack_pn);
    hash = fnv32_mix_opt_u64(hash, snapshot.congestion_window);
    hash = fnv32_mix_opt_u64(hash, snapshot.in_flight_bytes);
    match snapshot.algorithm {
        Some(crate::transport::TransportAlgorithm::Cubic) => fnv32_mix_u8(hash, 1),
        Some(crate::transport::TransportAlgorithm::Reno) => fnv32_mix_u8(hash, 2),
        Some(crate::transport::TransportAlgorithm::Other(code)) => {
            fnv32_mix_u8(fnv32_mix_u8(hash, 3), code)
        }
        None => fnv32_mix_u8(hash, 0),
    }
}

#[inline]
const fn saturating_u64_to_u32(value: Option<u64>) -> u32 {
    match value {
        Some(v) => {
            if v > u32::MAX as u64 {
                u32::MAX
            } else {
                v as u32
            }
        }
        None => 0,
    }
}

#[inline]
const fn opt_u32_or_zero(value: Option<u32>) -> u32 {
    match value {
        Some(v) => v,
        None => 0,
    }
}

/// Canonical replay transport inputs consumed by VM `GET_*` opcodes.
///
/// Returns `[latency_us, queue_depth, congestion_marks, retransmissions]`
/// with unavailable fields normalised to zero.
#[inline]
pub(crate) const fn replay_transport_inputs(snapshot: TransportSnapshot) -> [u32; 4] {
    [
        saturating_u64_to_u32(snapshot.latency_us),
        opt_u32_or_zero(snapshot.queue_depth),
        opt_u32_or_zero(snapshot.congestion_marks),
        opt_u32_or_zero(snapshot.retransmissions),
    ]
}

/// Presence bitmask for replay transport inputs.
///
/// bit0=latency, bit1=queue_depth, bit2=congestion_marks, bit3=retransmissions.
#[inline]
pub(crate) const fn replay_transport_presence(snapshot: TransportSnapshot) -> u8 {
    let mut mask = 0u8;
    if snapshot.latency_us.is_some() {
        mask |= 1 << 0;
    }
    if snapshot.queue_depth.is_some() {
        mask |= 1 << 1;
    }
    if snapshot.congestion_marks.is_some() {
        mask |= 1 << 2;
    }
    if snapshot.retransmissions.is_some() {
        mask |= 1 << 3;
    }
    mask
}

/// Unified action surface consumed by slot owners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Proceed,
    Abort(AbortInfo),
    Tap { id: u16, arg0: u32, arg1: u32 },
    Route { arm: u8 },
    Defer { retry_hint: u8 },
}

impl Action {
    /// Convert the full action stream into emergency verdicts.
    ///
    /// Tap is observational and therefore normalises to `Proceed`.
    pub(crate) const fn verdict(self) -> PolicyVerdict {
        match self {
            Action::Proceed => PolicyVerdict::Proceed,
            Action::Route { arm } if arm <= 1 => PolicyVerdict::RouteArm(arm),
            Action::Route { .. } => PolicyVerdict::Reject(ENGINE_FAIL_CLOSED),
            Action::Abort(info) => PolicyVerdict::Reject(info.reason),
            Action::Tap { .. } => PolicyVerdict::Proceed,
            Action::Defer { .. } => PolicyVerdict::Proceed,
        }
    }

    /// Apply runtime mode to policy action.
    ///
    /// Shadow mode never enforces policy decisions on the control/data path.
    /// Tap remains observable to preserve audit visibility.
    pub(crate) const fn with_mode(self, mode: PolicyMode) -> Self {
        match mode {
            PolicyMode::Enforce => self,
            PolicyMode::Shadow => match self {
                Action::Tap { .. } => self,
                _ => Action::Proceed,
            },
        }
    }
}

/// Execute the VM with an opportunity to configure the [`VmCtx`] prior to dispatch.
pub(crate) fn run_with<F>(
    host_slots: &HostSlots<'_>,
    slot: Slot,
    event: &TapEvent,
    caps: CapsMask,
    session: Option<SessionId>,
    lane: Option<Lane>,
    configure: F,
) -> Action
where
    F: FnOnce(&mut VmCtx<'_>),
{
    let vm_action = host_slots.execute_with(slot, event, caps, session, lane, configure);
    let action = convert_action(vm_action);
    action.with_mode(host_slots.policy_mode(slot))
}

fn convert_action(vm_action: VmAction) -> Action {
    match vm_action {
        VmAction::Proceed => Action::Proceed,
        VmAction::Abort { reason } => Action::Abort(AbortInfo { reason, trap: None }),
        VmAction::Trap(trap) => Action::Abort(AbortInfo {
            reason: ENGINE_FAIL_CLOSED,
            trap: Some(trap),
        }),
        VmAction::Tap { id, arg0, arg1 } => Action::Tap { id, arg0, arg1 },
        VmAction::Route { arm } => Action::Route { arm },
        VmAction::Defer { retry_hint } => Action::Defer { retry_hint },
        VmAction::Ra(_) => Action::Abort(AbortInfo {
            reason: ENGINE_FAIL_CLOSED,
            trap: Some(Trap::IllegalSyscall),
        }),
    }
}

#[cfg(test)]
#[path = "epf/policy_replay_tests.rs"]
mod policy_replay_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{control::cap::mint::CapsMask, epf::ops, observe::events::RawEvent};

    #[test]
    fn effect_syscall_is_fail_closed() {
        let code = [ops::instr::ACT_EFFECT, ops::effect::CHECKPOINT, 0x00];
        let scratch = std::boxed::Box::leak(std::vec![0u8; 64].into_boxed_slice());
        let machine =
            super::host::Machine::with_mem(&code, scratch, scratch.len(), 16).expect("machine");
        let mut slots = HostSlots::new();
        slots.install(Slot::Rendezvous, machine).expect("install");

        let action = run_with(
            &slots,
            Slot::Rendezvous,
            &RawEvent::zero(),
            CapsMask::allow_all(),
            None,
            None,
            |_| {},
        );
        assert!(matches!(
            action,
            Action::Abort(AbortInfo {
                reason: ENGINE_FAIL_CLOSED,
                trap: Some(Trap::IllegalSyscall),
            })
        ));
    }
}
