//! EPF VM bytecode interpreter (no_std / no_alloc).
//!
//! The interpreter executes bytecode verified by `crate::epf::verifier`, one
//! instruction at a time, and emits [`VmAction`] values that the host applies to
//! the rendezvous/control plane. Each instruction consumes one unit of fuel;
//! running out of fuel traps with [`Trap::FuelExhausted`]. The register file is
//! reset on each invocation, while the scratch memory slice (`mem`) persists
//! between runs so that hosts can keep small state across hooks.
//!
//! # Instruction encoding
//!
//! All opcodes are single-byte discriminants followed by little-endian
//! operands. The interpreter currently supports the following instructions:
//!
//! | Opcode | Mnemonic                        | Encoding (suffix)        | Description |
//! | ------ | ------------------------------- | ------------------------ | ----------- |
//! | `0x00` | `NOP`                           | —                        | No-op. |
//! | `0x01` | `HALT`                          | —                        | Stop execution and return [`VmAction::Proceed`]. |
//! | `0x10` | `LOAD_IMM rd, imm32`            | `dest:u8, imm:le u32`    | Load immediate into register `rd`. |
//! | `0x11` | `JUMP imm16`                    | `target:le u16`          | Jump to absolute byte offset `target`. |
//! | `0x12` | `JUMP_Z rs, imm16`              | `rs:u8, target:le u16`   | Jump if register `rs` is zero. |
//! | `0x13` | `JUMP_GT rs, rt, imm16`         | `rs:u8, rt:u8, target:le u16` | Jump if `rs` > `rt`. |
//! | `0x20` | `LOAD_MEM rd, rs`               | `dest:u8, addr:u8`       | Load one byte from `mem[addr]` into `rd`. |
//! | `0x21` | `STORE_MEM rs, rd`              | `src:u8, addr:u8`        | Store low 8 bits of `rs` into `mem[addr]`. |
//! | `0x30` | `ACT_EFFECT op, rs`             | `effect:u8, arg:u8`      | Dispatch control-plane effect (operand taken from `rs`). |
//! | `0x31` | `ACT_ABORT imm16`               | `reason:le u16`          | Emit [`VmAction::Abort { reason }`]. |
//! | `0x32` | `ACT_ANNOT key, rs`             | `key:le u16, val:u8`     | Store annotation in [`VmCtx`] (non-terminal), value from `rs`. |
//! | `0x33` | `ACT_ROUTE rs`                  | `rs:u8`                  | Return route arm from `rs` and terminate. |
//! | `0x34` | `ACT_DEFER rs`                  | `rs:u8`                  | Return defer retry hint from `rs` and terminate. |
//! | `0x40` | `GET_LATENCY rd`                | `dest:u8`                | Load the current transport latency (µs, saturated to `u32`) into `rd`. |
//! | `0x41` | `GET_QUEUE rd`                  | `dest:u8`                | Load the current queue depth (frames) into `rd`. |
//! | `0x43` | `GET_CONGESTION rd`             | `dest:u8`                | Load the observed congestion mark count into `rd`. |
//! | `0x44` | `GET_RETRY rd`                  | `dest:u8`                | Load the observed retransmission count into `rd`. |
//! | `0x45` | `GET_SCOPE_RANGE rd`            | `dest:u8`                | Load the current scope range ordinal (or `0` if unavailable). |
//! | `0x46` | `GET_SCOPE_NEST rd`             | `dest:u8`                | Load the current scope nest ordinal (or `0` if unavailable). |
//! | `0x47` | `TAP_OUT id, rs, rt`            | `id:le u16, rs:u8, rt:u8`| Emit [`VmAction::Tap`] with custom `id` and register values. |
//! | `0x48` | `GET_EVENT_ID rd`               | `dest:u8`                | Load the triggering event's id (u16) into `rd`. |
//! | `0x49` | `GET_EVENT_ARG0 rd`             | `dest:u8`                | Load the triggering event's arg0 into `rd`. |
//! | `0x4A` | `GET_EVENT_ARG1 rd`             | `dest:u8`                | Load the triggering event's arg1 into `rd`. |
//! | `0x4B` | `GET_INPUT rd, imm8`            | `dest:u8, idx:u8`        | Load `policy_input[idx]` into `rd` (`idx: 0..=3`). |
//! | `0x50` | `SHR rd, rs, imm8`              | `dest:u8, src:u8, shift:u8` | Shift right: `rd = rs >> imm8` (masked to 0-31). |
//! | `0x51` | `AND rd, rs, rt`                | `dest:u8, src1:u8, src2:u8` | Bitwise AND: `rd = rs & rt`. |
//!
//! Unrecognised opcodes trap with [`Trap::IllegalOpcode`]. Accessing memory
//! outside `mem` traps with [`Trap::OutOfBounds`]. Invalid instruction layouts
//! (e.g. truncated operands, register indices outside `0..REG_COUNT`) trap
//! with [`Trap::VerifyFailed`]; the verifier prevents these in well-formed
//! bytecode, but the interpreter defends against malformed images.

#[cfg(not(test))]
use core::marker::PhantomData;

#[cfg(test)]
use crate::observe::scope::{ScopeTrace, tap_scope};
use crate::{
    control::cap::mint::CapsMask,
    control::types::{Lane, SessionId},
    observe::core::TapEvent,
    transport::TransportSnapshot,
};

#[cfg(test)]
use super::{
    dispatch::{self, RaOp, SyscallError},
    ops,
};

#[cfg(test)]
const REG_COUNT: usize = 8;

/// Execution slot defines which subsystem triggered the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Forward,
    EndpointRx,
    EndpointTx,
    Rendezvous,
    Route,
}

/// Trap reasons emitted by the interpreter.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trap {
    FuelExhausted,
    IllegalOpcode(u8),
    OutOfBounds,
    IllegalSyscall,
    VerifyFailed,
}

/// Trap reasons emitted by the interpreter.
#[cfg(not(test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trap {}

/// Host-facing action emitted by the VM.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VmAction {
    Proceed,
    Abort {
        reason: u16,
    },
    /// Execute a control-plane effect permitted by the current capability set.
    Ra(RaOp),
    Trap(Trap),
    Tap {
        id: u16,
        arg0: u32,
        arg1: u32,
    },
    /// Return a route arm decision from the Route slot.
    Route {
        arm: u8,
    },
    /// Request route re-evaluation from the Route slot.
    Defer {
        retry_hint: u8,
    },
}

/// Maximum annotations that can be stored per VM invocation.
#[cfg(test)]
pub(crate) const ANNOT_CAP: usize = 4;

/// A single annotation entry (key-value pair).
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Annotation {
    pub key: u16,
    pub val: u32,
}

/// Execution context punctured into the interpreter.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct VmCtx<'a> {
    slot: Slot,
    event: &'a TapEvent,
    caps: CapsMask,
    session: Option<SessionId>,
    lane: Option<Lane>,
    scope: Option<ScopeTrace>,
    transport: TransportSnapshot,
    policy_input: [u32; 4],
    annotations: [Annotation; ANNOT_CAP],
    annot_len: u8,
    annot_cnt: u8,
}

/// Execution context punctured into the interpreter.
#[cfg(not(test))]
#[derive(Debug, Default)]
pub(crate) struct VmCtx<'a> {
    _marker: PhantomData<&'a TapEvent>,
}

#[cfg(test)]
impl<'a> VmCtx<'a> {
    pub(crate) fn new(slot: Slot, event: &'a TapEvent, caps: CapsMask) -> Self {
        Self {
            slot,
            event,
            caps,
            session: None,
            lane: None,
            scope: tap_scope(event),
            transport: TransportSnapshot::default(),
            policy_input: [0; 4],
            annotations: [Annotation::default(); ANNOT_CAP],
            annot_len: 0,
            annot_cnt: 0,
        }
    }

    /// Attach the session identifier for this invocation.
    #[inline]
    pub(crate) fn set_session(&mut self, session: SessionId) {
        self.session = Some(session);
    }

    /// Attach the lane identifier for this invocation.
    #[inline]
    pub(crate) fn set_lane(&mut self, lane: Lane) {
        self.lane = Some(lane);
    }

    /// Attach transport metrics snapshot for this invocation.
    #[inline]
    pub(crate) fn set_transport_snapshot(&mut self, snapshot: TransportSnapshot) {
        self.transport = snapshot;
    }

    /// Attach policy input arguments for this invocation.
    #[inline]
    pub(crate) fn set_policy_input(&mut self, input: [u32; 4]) {
        self.policy_input = input;
    }

    /// Retrieve the currently attached transport metrics snapshot.
    #[inline]
    pub(crate) fn transport_snapshot(&self) -> TransportSnapshot {
        self.transport
    }

    /// Validate that the VM is authorised to emit the given [`crate::control::cluster::effects::CpEffect`] and return it.
    #[inline]
    pub(crate) fn ensure_effect(&self, call: RaOp) -> Result<RaOp, SyscallError> {
        dispatch::ensure_allowed(self.slot, self.caps, call)
    }

    /// Scope trace associated with the triggering tap, when present.
    #[inline]
    pub(crate) fn scope_trace(&self) -> Option<ScopeTrace> {
        self.scope
    }

    /// Record an annotation (non-terminal side effect).
    ///
    /// When the buffer is full, increments the saturation counter without storing.
    #[inline]
    pub(crate) fn push_annotation(&mut self, key: u16, val: u32) {
        let idx = self.annot_len as usize;
        if idx < ANNOT_CAP {
            self.annotations[idx] = Annotation { key, val };
            self.annot_len += 1;
        }
        self.annot_cnt = self.annot_cnt.saturating_add(1);
    }

    /// Returns the stored annotations (up to [`ANNOT_CAP`]).
    #[cfg(test)]
    #[inline]
    pub(crate) fn annotations(&self) -> &[Annotation] {
        &self.annotations[..self.annot_len as usize]
    }

    /// Total number of `ACT_ANNOT` calls during this execution (may exceed buffer size).
    #[cfg(test)]
    #[inline]
    pub(crate) fn annot_count(&self) -> u8 {
        self.annot_cnt
    }

    /// Returns `true` if some annotations were dropped due to buffer saturation.
    #[cfg(test)]
    #[inline]
    pub(crate) fn annot_dropped(&self) -> bool {
        self.annot_cnt as usize > ANNOT_CAP
    }
}

#[cfg(not(test))]
impl<'a> VmCtx<'a> {
    pub(crate) fn new(_slot: Slot, _event: &'a TapEvent, _caps: CapsMask) -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn set_session(&mut self, _session: SessionId) {}

    #[inline]
    pub(crate) fn set_lane(&mut self, _lane: Lane) {}

    #[inline]
    pub(crate) fn set_transport_snapshot(&mut self, _snapshot: TransportSnapshot) {}

    #[inline]
    pub(crate) fn set_policy_input(&mut self, _input: [u32; 4]) {}
}

/// Opaque VM instance (bytecode + scratch buffers).
#[cfg(test)]
pub(crate) struct Vm<'code> {
    pub(crate) code: &'code [u8],
    pub(crate) fuel: u16,
    pub(crate) mem: &'code mut [u8],
}

#[cfg(test)]
impl<'code> Vm<'code> {
    pub(crate) fn new(code: &'code [u8], mem: &'code mut [u8], fuel: u16) -> Self {
        Self { code, fuel, mem }
    }

    pub(crate) fn execute<'a>(&mut self, ctx: &mut VmCtx<'a>) -> VmAction {
        let mut regs = [0u32; REG_COUNT];
        let mut pc = 0usize;
        let mut fuel = self.fuel;

        let code = self.code;

        loop {
            if fuel == 0 {
                self.fuel = 0;
                return VmAction::Trap(Trap::FuelExhausted);
            }

            if pc >= code.len() {
                self.fuel = fuel;
                return VmAction::Trap(Trap::VerifyFailed);
            }

            fuel -= 1;
            let opcode = code[pc];
            pc += 1;

            match opcode {
                ops::instr::NOP => {}
                ops::instr::HALT => {
                    self.fuel = fuel;
                    return VmAction::Proceed;
                }
                ops::instr::LOAD_IMM => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let imm = match read_u32(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = imm;
                }
                ops::instr::JUMP => {
                    let target = match read_u16(code, &mut pc) {
                        Some(val) => val as usize,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    if target >= code.len() {
                        self.fuel = fuel;
                        return VmAction::Trap(Trap::VerifyFailed);
                    }
                    pc = target;
                }
                ops::instr::JUMP_Z => {
                    let reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let target = match read_u16(code, &mut pc) {
                        Some(val) => val as usize,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    if target >= code.len() {
                        self.fuel = fuel;
                        return VmAction::Trap(Trap::VerifyFailed);
                    }
                    if regs[reg] == 0 {
                        pc = target;
                    }
                }
                ops::instr::JUMP_GT => {
                    let rs = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let rt = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let target = match read_u16(code, &mut pc) {
                        Some(val) => val as usize,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    if target >= code.len() {
                        self.fuel = fuel;
                        return VmAction::Trap(Trap::VerifyFailed);
                    }
                    if regs[rs] > regs[rt] {
                        pc = target;
                    }
                }
                ops::instr::LOAD_MEM => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let addr_reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let addr = regs[addr_reg] as usize;
                    match self.mem.get(addr) {
                        Some(byte) => regs[dest] = *byte as u32,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::OutOfBounds);
                        }
                    }
                }
                ops::instr::STORE_MEM => {
                    let src = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let addr_reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let addr = regs[addr_reg] as usize;
                    if let Some(slot) = self.mem.get_mut(addr) {
                        *slot = (regs[src] & 0xFF) as u8;
                    } else {
                        self.fuel = fuel;
                        return VmAction::Trap(Trap::OutOfBounds);
                    }
                }
                ops::instr::GET_LATENCY => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = encode_latency(ctx.transport_snapshot().latency_us);
                }
                ops::instr::GET_QUEUE => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.transport_snapshot().queue_depth.unwrap_or(0);
                }
                ops::instr::GET_CONGESTION => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.transport_snapshot().congestion_marks.unwrap_or(0);
                }
                ops::instr::GET_RETRY => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.transport_snapshot().retransmissions.unwrap_or(0);
                }
                ops::instr::GET_SCOPE_RANGE => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx
                        .scope_trace()
                        .map(|trace| trace.range as u32)
                        .unwrap_or(0);
                }
                ops::instr::GET_SCOPE_NEST => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx
                        .scope_trace()
                        .map(|trace| trace.nest as u32)
                        .unwrap_or(0);
                }
                ops::instr::GET_EVENT_ID => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.event.id as u32;
                }
                ops::instr::GET_EVENT_ARG0 => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.event.arg0;
                }
                ops::instr::GET_EVENT_ARG1 => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = ctx.event.arg1;
                }
                ops::instr::GET_INPUT => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let idx = match read_u8(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    if idx > 3 {
                        self.fuel = fuel;
                        return VmAction::Trap(Trap::VerifyFailed);
                    }
                    regs[dest] = ctx.policy_input[idx as usize];
                }
                ops::instr::SHR => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let src = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let shift = match read_u8(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = regs[src] >> (shift & 0x1F);
                }
                ops::instr::AND => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let src1 = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let src2 = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = regs[src1] & regs[src2];
                }
                ops::instr::JUMP_EQ_IMM => {
                    let src = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let imm = match read_u8(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let target = match read_u16(code, &mut pc) {
                        Some(val) => val as usize,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    if regs[src] == imm as u32 {
                        if target >= code.len() {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::OutOfBounds);
                        }
                        pc = target;
                    }
                }
                ops::instr::AND_IMM => {
                    let dest = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let src = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let imm = match read_u8(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    regs[dest] = regs[src] & (imm as u32);
                }
                ops::instr::ACT_EFFECT => {
                    let effect_opcode = match read_u8(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let arg_reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let arg = regs[arg_reg];
                    let call = match dispatch::decode_effect_call(effect_opcode, arg) {
                        Ok(call) => call,
                        Err(SyscallError::UnknownEffectOpcode(op)) => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::IllegalOpcode(op));
                        }
                        Err(SyscallError::NotAuthorised { .. }) => {
                            // Not expected from decode; treat as illegal syscall defensively.
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::IllegalSyscall);
                        }
                    };
                    match ctx.ensure_effect(call) {
                        Ok(op) => {
                            self.fuel = fuel;
                            return VmAction::Ra(op);
                        }
                        Err(SyscallError::NotAuthorised { .. }) => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::IllegalSyscall);
                        }
                        Err(SyscallError::UnknownEffectOpcode(op)) => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::IllegalOpcode(op));
                        }
                    }
                }
                ops::instr::ACT_ABORT => {
                    let reason = match read_u16(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    self.fuel = fuel;
                    return VmAction::Abort { reason };
                }
                ops::instr::ACT_ANNOT => {
                    let key = match read_u16(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let val = regs[reg];
                    ctx.push_annotation(key, val);
                    // Non-terminal: continue to next instruction
                }
                ops::instr::ACT_ROUTE => {
                    let reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let arm = (regs[reg] & 0xFF) as u8;
                    self.fuel = fuel;
                    return VmAction::Route { arm };
                }
                ops::instr::ACT_DEFER => {
                    let reg = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let retry_hint = (regs[reg] & 0xFF) as u8;
                    self.fuel = fuel;
                    return VmAction::Defer { retry_hint };
                }
                ops::instr::TAP_OUT => {
                    let id = match read_u16(code, &mut pc) {
                        Some(val) => val,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let rs = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    let rt = match read_reg(code, &mut pc) {
                        Some(idx) => idx,
                        None => {
                            self.fuel = fuel;
                            return VmAction::Trap(Trap::VerifyFailed);
                        }
                    };
                    self.fuel = fuel;
                    return VmAction::Tap {
                        id,
                        arg0: regs[rs],
                        arg1: regs[rt],
                    };
                }
                other => {
                    self.fuel = fuel;
                    return VmAction::Trap(Trap::IllegalOpcode(other));
                }
            }
        }
    }
}

#[cfg(test)]
fn read_u8(code: &[u8], pc: &mut usize) -> Option<u8> {
    if *pc < code.len() {
        let byte = code[*pc];
        *pc += 1;
        Some(byte)
    } else {
        None
    }
}

#[cfg(test)]
fn read_u16(code: &[u8], pc: &mut usize) -> Option<u16> {
    let lo = read_u8(code, pc)? as u16;
    let hi = read_u8(code, pc)? as u16;
    Some(lo | (hi << 8))
}

#[cfg(test)]
fn read_u32(code: &[u8], pc: &mut usize) -> Option<u32> {
    let b0 = read_u8(code, pc)? as u32;
    let b1 = read_u8(code, pc)? as u32;
    let b2 = read_u8(code, pc)? as u32;
    let b3 = read_u8(code, pc)? as u32;
    Some(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
}

#[cfg(test)]
fn read_reg(code: &[u8], pc: &mut usize) -> Option<usize> {
    let idx = read_u8(code, pc)? as usize;
    if idx < REG_COUNT { Some(idx) } else { None }
}

#[cfg(test)]
fn encode_latency(value: Option<u64>) -> u32 {
    const MAX: u64 = u32::MAX as u64;
    value.map(|lat| lat.min(MAX) as u32).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::ops;
    use super::*;
    use crate::{
        observe::core::TapEvent,
        observe::{events::RawEvent, scope::ScopeTrace},
        transport::TransportSnapshot,
    };

    fn make_ctx(slot: Slot, caps: CapsMask) -> VmCtx<'static> {
        // Static event used for the lifetime requirement inside VmCtx.
        static EVENT: TapEvent = RawEvent::zero();
        VmCtx::new(slot, &EVENT, caps)
    }

    #[test]
    fn act_effect_returns_effect_action() {
        let code = [
            ops::instr::LOAD_IMM,
            0x01,
            0x07,
            0x00,
            0x00,
            0x00, // r1 = 7
            ops::instr::ACT_EFFECT,
            ops::effect::SPLICE_BEGIN,
            0x01, // effect 0x00 (SpliceBegin) with r1
        ];
        let mut mem = [0u8; 8];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = make_ctx(Slot::Rendezvous, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Ra(RaOp::SpliceBegin { arg: 7 }));
        assert_eq!(vm.fuel, 6);
    }

    #[test]
    fn fuel_exhaustion_traps() {
        let code = [ops::instr::NOP];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 1);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::FuelExhausted));
        assert_eq!(vm.fuel, 0);
    }

    #[test]
    fn illegal_opcode_traps() {
        let code = [0xFF];
        let mut mem = [0u8; 2];
        let mut vm = Vm::new(&code, &mut mem, 4);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::IllegalOpcode(0xFF)));
    }

    #[test]
    fn act_route_returns_arm() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x02,
            0x00,
            0x00,
            0x00, // r0 = 2
            ops::instr::ACT_ROUTE,
            0x00, // return route arm from r0
        ];
        let mut mem = [0u8; 8];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = make_ctx(Slot::Route, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Route { arm: 2 });
    }

    #[test]
    fn act_defer_returns_retry_hint() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x07,
            0x00,
            0x00,
            0x00, // r0 = 7
            ops::instr::ACT_DEFER,
            0x00, // return defer retry hint from r0
        ];
        let mut mem = [0u8; 8];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = make_ctx(Slot::Route, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Defer { retry_hint: 7 });
    }

    #[test]
    fn transport_metric_loaders_surface_extended_stats() {
        let code = [
            ops::instr::GET_CONGESTION,
            0x00,
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00, // r2 = 0 (mem index for congestion)
            ops::instr::STORE_MEM,
            0x00,
            0x02,
            ops::instr::GET_RETRY,
            0x01,
            ops::instr::LOAD_IMM,
            0x03,
            0x01,
            0x00,
            0x00,
            0x00, // r3 = 1 (mem index for retry)
            ops::instr::STORE_MEM,
            0x01,
            0x03,
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 16);
        let mut ctx = make_ctx(Slot::EndpointTx, CapsMask::allow_all());
        let snapshot = TransportSnapshot::new(Some(42), Some(9))
            .with_congestion_marks(Some(7))
            .with_retransmissions(Some(3));
        ctx.set_transport_snapshot(snapshot);
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(mem[0], 7);
        assert_eq!(mem[1], 3);
    }

    #[test]
    fn scope_loaders_surface_range_and_nest() {
        let scope = ScopeTrace::new(5, 9);
        let event = TapEvent {
            ts: 0,
            id: crate::observe::ids::ENDPOINT_SEND,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: scope.pack(),
        };
        let code = [
            ops::instr::GET_SCOPE_RANGE,
            0x00,
            ops::instr::GET_SCOPE_NEST,
            0x01,
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00, // r2 = 0
            ops::instr::STORE_MEM,
            0x00,
            0x02,
            ops::instr::LOAD_IMM,
            0x03,
            0x01,
            0x00,
            0x00,
            0x00, // r3 = 1
            ops::instr::STORE_MEM,
            0x01,
            0x03,
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 12);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(mem[0], scope.range as u8);
        assert_eq!(mem[1], scope.nest as u8);
    }

    #[test]
    fn scope_loaders_return_zero_without_metadata() {
        let event = TapEvent {
            ts: 0,
            id: crate::observe::ids::ENDPOINT_SEND,
            causal_key: 0,
            arg0: 0,
            arg1: 0,
            arg2: 0,
        };
        let code = [
            ops::instr::GET_SCOPE_RANGE,
            0x00,
            ops::instr::GET_SCOPE_NEST,
            0x01,
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00,
            ops::instr::STORE_MEM,
            0x00,
            0x02,
            ops::instr::LOAD_IMM,
            0x03,
            0x01,
            0x00,
            0x00,
            0x00,
            ops::instr::STORE_MEM,
            0x01,
            0x03,
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 12);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(mem[0], 0);
        assert_eq!(mem[1], 0);
    }

    #[test]
    fn vm_ctx_transport_snapshot_roundtrip() {
        let mut ctx = make_ctx(Slot::Rendezvous, CapsMask::allow_all());
        assert_eq!(ctx.transport_snapshot(), TransportSnapshot::default());
        let snapshot = TransportSnapshot::new(Some(42), Some(7));
        ctx.set_transport_snapshot(snapshot);
        assert_eq!(ctx.transport_snapshot(), snapshot);
    }

    #[test]
    fn memory_out_of_bounds_traps() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x08,
            0x00,
            0x00,
            0x00, // r0 = 8 (addr)
            ops::instr::LOAD_IMM,
            0x01,
            0xAB,
            0x00,
            0x00,
            0x00, // r1 = 0xAB
            ops::instr::STORE_MEM,
            0x01,
            0x00, // mem[r0] = r1 (out of bounds)
        ];
        let mut mem = [0u8; 8];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::OutOfBounds));
    }

    #[test]
    fn illegal_syscall_traps() {
        let code = [
            ops::instr::ACT_EFFECT,
            ops::effect::CHECKPOINT,
            0x00, // Checkpoint from r0 (default 0)
        ];
        let mut mem = [0u8; 2];
        let mut vm = Vm::new(&code, &mut mem, 4);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::IllegalSyscall));
    }

    #[test]
    fn abort_action() {
        let code = [
            ops::instr::ACT_ABORT,
            0x39,
            0x30, // reason = 0x3039
        ];
        let mut mem = [0u8; 1];
        let mut vm = Vm::new(&code, &mut mem, 4);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let abort = vm.execute(&mut ctx);
        assert_eq!(abort, VmAction::Abort { reason: 0x3039 });
    }

    #[test]
    fn annot_stores_in_ctx_non_terminal() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x2A,
            0x00,
            0x00,
            0x00, // r0 = 42
            ops::instr::ACT_ANNOT,
            0x34,
            0x12,
            0x00, // annotate key 0x1234 with r0
            ops::instr::LOAD_IMM,
            0x01,
            0x63,
            0x00,
            0x00,
            0x00, // r1 = 99
            ops::instr::ACT_ANNOT,
            0x78,
            0x56,
            0x01, // annotate key 0x5678 with r1
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 16);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(ctx.annot_count(), 2);
        assert!(!ctx.annot_dropped());
        let annots = ctx.annotations();
        assert_eq!(annots.len(), 2);
        assert_eq!(
            annots[0],
            Annotation {
                key: 0x1234,
                val: 42
            }
        );
        assert_eq!(
            annots[1],
            Annotation {
                key: 0x5678,
                val: 99
            }
        );
    }

    #[test]
    fn annot_saturation_detectable() {
        // 6 annotations: 4 stored, 2 dropped (count=6)
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00, // r0 = 1
            ops::instr::ACT_ANNOT,
            0x01,
            0x00,
            0x00, // key=1
            ops::instr::LOAD_IMM,
            0x00,
            0x02,
            0x00,
            0x00,
            0x00, // r0 = 2
            ops::instr::ACT_ANNOT,
            0x02,
            0x00,
            0x00, // key=2
            ops::instr::LOAD_IMM,
            0x00,
            0x03,
            0x00,
            0x00,
            0x00, // r0 = 3
            ops::instr::ACT_ANNOT,
            0x03,
            0x00,
            0x00, // key=3
            ops::instr::LOAD_IMM,
            0x00,
            0x04,
            0x00,
            0x00,
            0x00, // r0 = 4
            ops::instr::ACT_ANNOT,
            0x04,
            0x00,
            0x00, // key=4
            ops::instr::LOAD_IMM,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00, // r0 = 5
            ops::instr::ACT_ANNOT,
            0x05,
            0x00,
            0x00, // key=5 (dropped)
            ops::instr::LOAD_IMM,
            0x00,
            0x06,
            0x00,
            0x00,
            0x00, // r0 = 6
            ops::instr::ACT_ANNOT,
            0x06,
            0x00,
            0x00, // key=6 (dropped)
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 32);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(ctx.annot_count(), 6);
        assert!(ctx.annot_dropped());
        assert_eq!(ctx.annotations().len(), ANNOT_CAP);
        // First 4 stored
        assert_eq!(ctx.annotations()[0].key, 1);
        assert_eq!(ctx.annotations()[3].key, 4);
    }

    // =========================================================================
    // Tests for new opcodes: GET_EVENT_ARG0, GET_EVENT_ARG1, GET_INPUT, SHR, AND, AND_IMM, JUMP_EQ_IMM
    // =========================================================================

    #[test]
    fn get_event_arg0_loads_arg0() {
        // Create event with known arg0 value
        let event = crate::observe::events::RawEvent::new(0, 0x1234)
            .with_arg0(0xDEADBEEF)
            .with_arg1(0xCAFEBABE);
        // Use TAP_OUT to verify the loaded value (STORE_MEM only stores 1 byte)
        let code = [
            ops::instr::GET_EVENT_ARG0,
            0x00, // r0 = arg0
            ops::instr::TAP_OUT,
            0x99,
            0x00, // id = 0x0099
            0x00, // rs = r0
            0x01, // rt = r1 (0)
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(
            action,
            VmAction::Tap {
                id: 0x0099,
                arg0: 0xDEADBEEF,
                arg1: 0
            }
        );
    }

    #[test]
    fn get_event_arg1_loads_arg1() {
        // Create event with known arg1 value
        let event = crate::observe::events::RawEvent::new(0, 0x1234)
            .with_arg0(0xDEADBEEF)
            .with_arg1(0xCAFEBABE);
        // Use TAP_OUT to verify the loaded value
        let code = [
            ops::instr::GET_EVENT_ARG1,
            0x00, // r0 = arg1
            ops::instr::TAP_OUT,
            0x99,
            0x00, // id = 0x0099
            0x00, // rs = r0
            0x01, // rt = r1 (0)
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(
            action,
            VmAction::Tap {
                id: 0x0099,
                arg0: 0xCAFEBABE,
                arg1: 0
            }
        );
    }

    #[test]
    fn get_input_loads_input_arg() {
        let event = crate::observe::events::RawEvent::new(0, 0x1234)
            .with_arg0(0xDEADBEEF)
            .with_arg1(0xCAFEBABE);
        let code = [
            ops::instr::GET_INPUT,
            0x00, // r0
            0x02, // input arg index 2
            ops::instr::TAP_OUT,
            0x99,
            0x00, // id = 0x0099
            0x00, // rs = r0
            0x01, // rt = r1 (0)
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        ctx.set_policy_input([11, 22, 33, 44]);
        let action = vm.execute(&mut ctx);
        assert_eq!(
            action,
            VmAction::Tap {
                id: 0x0099,
                arg0: 33,
                arg1: 0
            }
        );
    }

    #[test]
    fn get_input_with_invalid_index_traps_verify_failed() {
        let event = crate::observe::events::RawEvent::new(0, 0x1234)
            .with_arg0(0xDEADBEEF)
            .with_arg1(0xCAFEBABE);
        let code = [
            ops::instr::GET_INPUT,
            0x00, // r0
            0x04, // invalid index
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 8);
        let mut ctx = VmCtx::new(Slot::EndpointTx, &event, CapsMask::allow_all());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::VerifyFailed));
    }

    #[test]
    fn shr_shifts_right() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x00,
            0xFF,
            0x00,
            0x00, // r0 = 0xFF00
            ops::instr::SHR,
            0x01,
            0x00,
            8, // r1 = r0 >> 8
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00, // r2 = 0 (mem index)
            ops::instr::STORE_MEM,
            0x01,
            0x02, // mem[0] = r1
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(u32::from_le_bytes(mem), 0xFF); // 0xFF00 >> 8 = 0xFF
    }

    #[test]
    fn shr_masks_shift_amount() {
        // Shift by 33 should be masked to 1 (33 & 0x1F = 1)
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x04,
            0x00,
            0x00,
            0x00, // r0 = 4
            ops::instr::SHR,
            0x01,
            0x00,
            33, // r1 = r0 >> 33 (actually >> 1)
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00, // r2 = 0 (mem index)
            ops::instr::STORE_MEM,
            0x01,
            0x02, // mem[0] = r1
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(u32::from_le_bytes(mem), 2); // 4 >> 1 = 2
    }

    #[test]
    fn and_bitwise_and() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0xFF,
            0x0F,
            0x00,
            0x00, // r0 = 0x0FFF
            ops::instr::LOAD_IMM,
            0x01,
            0xF0,
            0xF0,
            0x00,
            0x00, // r1 = 0xF0F0
            ops::instr::AND,
            0x02,
            0x00,
            0x01, // r2 = r0 & r1
            ops::instr::LOAD_IMM,
            0x03,
            0x00,
            0x00,
            0x00,
            0x00, // r3 = 0 (mem index)
            ops::instr::STORE_MEM,
            0x02,
            0x03, // mem[0] = r2
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 12);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(u32::from_le_bytes(mem), 0x0FFF & 0xF0F0); // 0x00F0
    }

    #[test]
    fn and_imm_bitwise_and_with_immediate() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0xAB,
            0xCD,
            0x00,
            0x00, // r0 = 0xCDAB
            ops::instr::AND_IMM,
            0x01,
            0x00,
            0xFF, // r1 = r0 & 0xFF
            ops::instr::LOAD_IMM,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00, // r2 = 0 (mem index)
            ops::instr::STORE_MEM,
            0x01,
            0x02, // mem[0] = r1
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        assert_eq!(u32::from_le_bytes(mem), 0xAB); // 0xCDAB & 0xFF = 0xAB
    }

    #[test]
    fn jump_eq_imm_jumps_when_equal() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00, // r0 = 5
            // offset 6: JUMP_EQ_IMM r0, 5, 15
            ops::instr::JUMP_EQ_IMM,
            0x00,
            0x05,
            15,
            0x00, // if r0 == 5, jump to offset 15
            // offset 11: LOAD_IMM r1, 0xBAD (should be skipped)
            ops::instr::LOAD_IMM,
            0x01,
            0xAD,
            0x0B,
            0x00,
            0x00,
            // offset 17: should not reach here if jump doesn't work
            // Let's put HALT at offset 15 to terminate
        ];
        // Add HALT at offset 15
        let mut full_code = [0u8; 20];
        full_code[..17].copy_from_slice(&code);
        full_code[15] = ops::instr::HALT;
        full_code[16] = ops::instr::HALT; // fallthrough

        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&full_code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        // If jump worked, we should have executed 3 instructions: LOAD_IMM, JUMP_EQ_IMM, HALT
        // fuel started at 10, 3 instructions = fuel 7
        assert_eq!(vm.fuel, 7);
    }

    #[test]
    fn jump_eq_imm_no_jump_when_not_equal() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00, // r0 = 5
            // offset 6: JUMP_EQ_IMM r0, 7, 20 (won't jump because 5 != 7)
            ops::instr::JUMP_EQ_IMM,
            0x00,
            0x07,
            20,
            0x00,
            // offset 11: HALT (should be reached)
            ops::instr::HALT,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Proceed);
        // LOAD_IMM, JUMP_EQ_IMM (no jump), HALT = 3 instructions, fuel 7
        assert_eq!(vm.fuel, 7);
    }

    #[test]
    fn jump_eq_imm_out_of_bounds_traps() {
        let code = [
            ops::instr::LOAD_IMM,
            0x00,
            0x05,
            0x00,
            0x00,
            0x00, // r0 = 5
            // offset 6: JUMP_EQ_IMM r0, 5, 100 (out of bounds)
            ops::instr::JUMP_EQ_IMM,
            0x00,
            0x05,
            100,
            0x00,
        ];
        let mut mem = [0u8; 4];
        let mut vm = Vm::new(&code, &mut mem, 10);
        let mut ctx = make_ctx(Slot::Forward, CapsMask::empty());
        let action = vm.execute(&mut ctx);
        assert_eq!(action, VmAction::Trap(Trap::OutOfBounds));
    }
}
