//! Shared opcode definitions for the EPF effect VM.
//!
//! Keeping instruction discriminants in a single location avoids accidental
//! drift between the interpreter, loader, and verifier. Host-side tooling
//! (e.g. the upcoming loader/management pipeline) should use these constants
//! whenever they need to reason about bytecode layout.

/// Byte-sized instruction opcodes understood by the interpreter.
pub(crate) mod instr {
    pub(crate) const NOP: u8 = 0x00;
    pub(crate) const HALT: u8 = 0x01;

    pub(crate) const LOAD_IMM: u8 = 0x10;
    pub(crate) const JUMP: u8 = 0x11;
    pub(crate) const JUMP_Z: u8 = 0x12;
    pub(crate) const JUMP_GT: u8 = 0x13;

    pub(crate) const LOAD_MEM: u8 = 0x20;
    pub(crate) const STORE_MEM: u8 = 0x21;

    pub(crate) const ACT_EFFECT: u8 = 0x30;
    pub(crate) const ACT_ABORT: u8 = 0x31;
    pub(crate) const ACT_ANNOT: u8 = 0x32;
    /// Return a route arm decision and terminate: `ACT_ROUTE rs:u8`.
    pub(crate) const ACT_ROUTE: u8 = 0x33;
    /// Request route re-evaluation and terminate: `ACT_DEFER rs:u8`.
    pub(crate) const ACT_DEFER: u8 = 0x34;

    pub(crate) const GET_LATENCY: u8 = 0x40;
    pub(crate) const GET_QUEUE: u8 = 0x41;
    pub(crate) const GET_CONGESTION: u8 = 0x43;
    pub(crate) const GET_RETRY: u8 = 0x44;
    pub(crate) const GET_SCOPE_RANGE: u8 = 0x45;
    pub(crate) const GET_SCOPE_NEST: u8 = 0x46;
    /// Emit a structured observation event: `TAP_OUT id:u16, rs, rt`.
    pub(crate) const TAP_OUT: u8 = 0x47;

    /// Load the triggering event's id (u16) into rd: `GET_EVENT_ID rd`.
    pub(crate) const GET_EVENT_ID: u8 = 0x48;
    /// Load the triggering event's arg0 into rd: `GET_EVENT_ARG0 rd`.
    pub(crate) const GET_EVENT_ARG0: u8 = 0x49;
    /// Load the triggering event's arg1 into rd: `GET_EVENT_ARG1 rd`.
    pub(crate) const GET_EVENT_ARG1: u8 = 0x4A;
    /// Load EPF input arg[index] into rd: `GET_INPUT rd, index`.
    pub(crate) const GET_INPUT: u8 = 0x4B;

    /// Shift right: `SHR rd, rs, imm8` — rd = rs >> imm8.
    pub(crate) const SHR: u8 = 0x50;
    /// Bitwise AND: `AND rd, rs, rt` — rd = rs & rt.
    pub(crate) const AND: u8 = 0x51;
    /// Jump if equal to immediate: `JUMP_EQ_IMM rs, imm8, target16` — if rs == imm8 then pc = target.
    pub(crate) const JUMP_EQ_IMM: u8 = 0x52;
    /// Bitwise AND with immediate: `AND_IMM rd, rs, imm8` — rd = rs & imm8.
    pub(crate) const AND_IMM: u8 = 0x53;
}

/// Opcodes used by `ACT_EFFECT` to identify control-plane calls.
pub(crate) mod effect {
    pub(crate) const SPLICE_BEGIN: u8 = 0x00;
    pub(crate) const SPLICE_COMMIT: u8 = 0x01;
    pub(crate) const SPLICE_ABORT: u8 = 0x02;
    pub(crate) const CHECKPOINT: u8 = 0x03;
    pub(crate) const ROLLBACK: u8 = 0x04;
}
