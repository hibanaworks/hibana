use crate::{control::cap::mint::CapsMask, control::cluster::effects::CpEffect, epf::vm::Slot};

use super::ops;

/// Rendezvous control operations surfaced by the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RaOp {
    /// Begin a splice operation.
    SpliceBegin { arg: u32 },
    /// Commit a splice operation.
    SpliceCommit { arg: u32 },
    /// Abort an in-flight splice.
    SpliceAbort { arg: u32 },
    /// Take a rendezvous checkpoint.
    Checkpoint,
    /// Roll back to a previously taken checkpoint.
    Rollback { generation: u32 },
}

impl RaOp {
    /// Control-plane effect produced by this call.
    #[inline]
    pub(crate) const fn effect(self) -> CpEffect {
        match self {
            RaOp::SpliceBegin { .. } => CpEffect::SpliceBegin,
            RaOp::SpliceCommit { .. } => CpEffect::SpliceCommit,
            RaOp::SpliceAbort { .. } => CpEffect::Abort,
            RaOp::Checkpoint => CpEffect::Checkpoint,
            RaOp::Rollback { .. } => CpEffect::Rollback,
        }
    }

    /// Capability required to execute this call.
    #[inline]
    pub(crate) const fn required_effect(self) -> CpEffect {
        self.effect()
    }
}

/// Errors surfaced by the dispatch layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyscallError {
    /// Attempted to invoke an unknown effect opcode.
    UnknownEffectOpcode(u8),
    /// Capability check failed for the given slot.
    NotAuthorised { slot: Slot, effect: CpEffect },
}

/// Map the byte-sized effect opcode used in bytecode to a concrete [`RaOp`].
pub(crate) fn decode_effect_call(op: u8, arg: u32) -> Result<RaOp, SyscallError> {
    let ra = match op {
        ops::effect::SPLICE_BEGIN => RaOp::SpliceBegin { arg },
        ops::effect::SPLICE_COMMIT => RaOp::SpliceCommit { arg },
        ops::effect::SPLICE_ABORT => RaOp::SpliceAbort { arg },
        ops::effect::CHECKPOINT => RaOp::Checkpoint,
        ops::effect::ROLLBACK => RaOp::Rollback { generation: arg },
        other => return Err(SyscallError::UnknownEffectOpcode(other)),
    };
    Ok(ra)
}

/// Validate that the requested control-plane effect is permitted for the given slot.
pub(crate) fn ensure_allowed(slot: Slot, caps: CapsMask, op: RaOp) -> Result<RaOp, SyscallError> {
    // Current policy: only the rendezvous slot may request control-plane effects.
    // Other slots are limited to Proceed/Abort/Tap annotations.
    if !matches!(slot, Slot::Rendezvous) {
        return Err(SyscallError::NotAuthorised {
            slot,
            effect: op.required_effect(),
        });
    }
    if !caps.allows(op.required_effect()) {
        return Err(SyscallError::NotAuthorised {
            slot,
            effect: op.required_effect(),
        });
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cluster::effects::CpEffect;

    #[test]
    fn capability_bits_roundtrip() {
        let caps = CapsMask::empty()
            .with(CpEffect::SpliceBegin)
            .with(CpEffect::Rollback);
        assert!(caps.allows(CpEffect::SpliceBegin));
        assert!(!caps.allows(CpEffect::SpliceCommit));
        assert!(caps.allows(CpEffect::Rollback));
    }

    #[test]
    fn raop_reports_cp_effect() {
        let begin = RaOp::SpliceBegin { arg: 7 };
        assert_eq!(begin.effect(), CpEffect::SpliceBegin);
        assert!(matches!(begin, RaOp::SpliceBegin { arg: 7 }));
        let checkpoint = RaOp::Checkpoint;
        assert_eq!(checkpoint.effect(), CpEffect::Checkpoint);
        assert!(matches!(checkpoint, RaOp::Checkpoint));
    }

    #[test]
    fn decode_known_ops() {
        assert!(matches!(
            decode_effect_call(0x00, 10).unwrap(),
            RaOp::SpliceBegin { arg: 10 }
        ));
        assert!(matches!(
            decode_effect_call(0x03, 0).unwrap(),
            RaOp::Checkpoint
        ));
        assert!(decode_effect_call(0xFF, 0).is_err());
    }

    #[test]
    fn reject_capability_mismatch() {
        let caps = CapsMask::empty();
        let op = RaOp::Checkpoint;
        let err = ensure_allowed(Slot::Rendezvous, caps, op).unwrap_err();
        assert!(matches!(err, SyscallError::NotAuthorised { .. }));
    }
}
