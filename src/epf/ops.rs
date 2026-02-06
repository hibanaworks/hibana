//! Shared opcode definitions for the EPF effect VM.
//!
//! Keeping instruction discriminants in a single location avoids accidental
//! drift between the interpreter, loader, and verifier. Host-side tooling
//! (e.g. the upcoming loader/management pipeline) should use these constants
//! whenever they need to reason about bytecode layout.

/// Byte-sized instruction opcodes understood by the interpreter.
pub mod instr {
    pub const NOP: u8 = 0x00;
    pub const HALT: u8 = 0x01;

    pub const LOAD_IMM: u8 = 0x10;
    pub const JUMP: u8 = 0x11;
    pub const JUMP_Z: u8 = 0x12;
    pub const JUMP_GT: u8 = 0x13;

    pub const LOAD_MEM: u8 = 0x20;
    pub const STORE_MEM: u8 = 0x21;

    pub const ACT_EFFECT: u8 = 0x30;
    pub const ACT_ABORT: u8 = 0x31;
    pub const ACT_ANNOT: u8 = 0x32;
    /// Return a route arm decision and terminate: `ACT_ROUTE rs:u8`.
    pub const ACT_ROUTE: u8 = 0x33;

    pub const GET_LATENCY: u8 = 0x40;
    pub const GET_QUEUE: u8 = 0x41;
    pub const GET_CONGESTION: u8 = 0x43;
    pub const GET_RETRY: u8 = 0x44;
    pub const GET_SCOPE_RANGE: u8 = 0x45;
    pub const GET_SCOPE_NEST: u8 = 0x46;
    /// Emit a structured observation event: `TAP_OUT id:u16, rs, rt`.
    pub const TAP_OUT: u8 = 0x47;

    /// Load the triggering event's id (u16) into rd: `GET_EVENT_ID rd`.
    pub const GET_EVENT_ID: u8 = 0x48;
    /// Load the triggering event's arg0 into rd: `GET_EVENT_ARG0 rd`.
    pub const GET_EVENT_ARG0: u8 = 0x49;
    /// Load the triggering event's arg1 into rd: `GET_EVENT_ARG1 rd`.
    pub const GET_EVENT_ARG1: u8 = 0x4A;

    /// Shift right: `SHR rd, rs, imm8` — rd = rs >> imm8.
    pub const SHR: u8 = 0x50;
    /// Bitwise AND: `AND rd, rs, rt` — rd = rs & rt.
    pub const AND: u8 = 0x51;
    /// Jump if equal to immediate: `JUMP_EQ_IMM rs, imm8, target16` — if rs == imm8 then pc = target.
    pub const JUMP_EQ_IMM: u8 = 0x52;
    /// Bitwise AND with immediate: `AND_IMM rd, rs, imm8` — rd = rs & imm8.
    pub const AND_IMM: u8 = 0x53;
}

/// Opcodes used by `ACT_EFFECT` to identify control-plane calls.
pub mod effect {
    pub const SPLICE_BEGIN: u8 = 0x00;
    pub const SPLICE_COMMIT: u8 = 0x01;
    pub const SPLICE_ABORT: u8 = 0x02;
    pub const CHECKPOINT: u8 = 0x03;
    pub const ROLLBACK: u8 = 0x04;
}
