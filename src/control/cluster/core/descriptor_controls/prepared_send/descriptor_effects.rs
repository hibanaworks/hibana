use super::descriptor_terminal::{DescriptorEffectTerminal, DescriptorTerminal};
use crate::control::cluster::core::{
    ControlCore, CpError, Generation, Lane, RendezvousId, SessionCluster, SessionId,
    StateRestoreError, TxAbortError, TxCommitError,
};

type ClusterCore<'cfg, T, U, C, const MAX_RV: usize> =
    ControlCore<'cfg, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV>;

fn map_state_restore_error(error: crate::rendezvous::error::StateRestoreError) -> CpError {
    let error = match error {
        crate::rendezvous::error::StateRestoreError::UnknownSession { .. } => {
            StateRestoreError::SessionNotFound
        }
        crate::rendezvous::error::StateRestoreError::NoStateSnapshot { .. } => {
            StateRestoreError::EpochNotFound
        }
        crate::rendezvous::error::StateRestoreError::StaleStateSnapshot { .. }
        | crate::rendezvous::error::StateRestoreError::EpochMismatch { .. } => {
            StateRestoreError::EpochMismatch
        }
        crate::rendezvous::error::StateRestoreError::AlreadyFinalized { .. } => {
            StateRestoreError::AlreadyFinalized
        }
    };
    CpError::StateRestore(error)
}

fn map_tx_commit_error(error: crate::rendezvous::error::TxCommitError) -> CpError {
    let error = match error {
        crate::rendezvous::error::TxCommitError::UnknownSession { .. } => {
            TxCommitError::SessionNotFound
        }
        crate::rendezvous::error::TxCommitError::NoStateSnapshot { .. } => {
            TxCommitError::NoStateSnapshot
        }
        crate::rendezvous::error::TxCommitError::AlreadyFinalized { .. } => {
            TxCommitError::AlreadyFinalized
        }
        crate::rendezvous::error::TxCommitError::GenerationMismatch { .. } => {
            TxCommitError::GenerationMismatch
        }
    };
    CpError::TxCommit(error)
}

fn map_tx_abort_error(error: crate::rendezvous::error::TxAbortError) -> CpError {
    let error = match error {
        crate::rendezvous::error::TxAbortError::UnknownSession { .. } => {
            TxAbortError::SessionNotFound
        }
        crate::rendezvous::error::TxAbortError::NoStateSnapshot { .. } => {
            TxAbortError::NoStateSnapshot
        }
        crate::rendezvous::error::TxAbortError::AlreadyFinalized { .. } => {
            TxAbortError::AlreadyFinalized
        }
        crate::rendezvous::error::TxAbortError::StaleStateSnapshot { .. }
        | crate::rendezvous::error::TxAbortError::GenerationMismatch { .. } => {
            TxAbortError::GenerationMismatch
        }
    };
    CpError::TxAbort(error)
}

impl<'cfg, T, U, C, const MAX_RV: usize> SessionCluster<'cfg, T, U, C, MAX_RV>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    #[inline(never)]
    pub(super) fn prepare_state_snapshot_descriptor_terminal(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner =
                core.locals
                    .owner_proof(rv_id)
                    .map_err(|_| CpError::RendezvousMismatch {
                        expected: rv_id.raw(),
                        actual: 0,
                    })?;
            let proof = core
                .locals
                .get_mut_by_proof(owner)
                .prepare_state_snapshot_effect(sid, lane, generation)
                .map_err(|err| CpError::Topology(err.into()))?;
            Ok(DescriptorTerminal::state_snapshot(owner, proof))
        })
    }

    #[inline(never)]
    pub(super) fn prepare_state_restore_descriptor_terminal(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner =
                core.locals
                    .owner_proof(rv_id)
                    .map_err(|_| CpError::RendezvousMismatch {
                        expected: rv_id.raw(),
                        actual: 0,
                    })?;
            let proof = core
                .locals
                .get_mut_by_proof(owner)
                .prepare_state_restore_effect(sid, lane, generation)
                .map_err(map_state_restore_error)?;
            Ok(DescriptorTerminal::state_restore(owner, proof))
        })
    }

    #[inline(never)]
    pub(super) fn prepare_tx_commit_descriptor_terminal(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner =
                core.locals
                    .owner_proof(rv_id)
                    .map_err(|_| CpError::RendezvousMismatch {
                        expected: rv_id.raw(),
                        actual: 0,
                    })?;
            let proof = core
                .locals
                .get_mut_by_proof(owner)
                .prepare_tx_commit_effect(sid, lane, generation)
                .map_err(map_tx_commit_error)?;
            Ok(DescriptorTerminal::tx_commit(owner, proof))
        })
    }

    #[inline(never)]
    pub(super) fn prepare_tx_abort_descriptor_terminal(
        &self,
        rv_id: RendezvousId,
        sid: SessionId,
        lane: Lane,
        generation: Generation,
    ) -> Result<DescriptorTerminal, CpError> {
        self.with_control_mut(|core| {
            let owner =
                core.locals
                    .owner_proof(rv_id)
                    .map_err(|_| CpError::RendezvousMismatch {
                        expected: rv_id.raw(),
                        actual: 0,
                    })?;
            let proof = core
                .locals
                .get_mut_by_proof(owner)
                .prepare_tx_abort_effect(sid, lane, generation)
                .map_err(map_tx_abort_error)?;
            Ok(DescriptorTerminal::tx_abort(owner, proof))
        })
    }

    #[inline(never)]
    pub(super) fn publish_descriptor_effect_terminal(&self, ticket: DescriptorEffectTerminal) {
        self.with_control_mut(|core| {
            Self::publish_descriptor_effect_terminal_in_core(core, ticket);
        });
    }

    #[inline(never)]
    pub(super) fn publish_descriptor_effect_terminal_in_core(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        ticket: DescriptorEffectTerminal,
    ) {
        match ticket {
            DescriptorEffectTerminal::StateSnapshot(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .publish_prepared_state_snapshot_effect(proof);
            }
            DescriptorEffectTerminal::StateRestore(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .publish_prepared_state_restore_effect(proof);
            }
            DescriptorEffectTerminal::TxCommit(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .publish_prepared_tx_commit_effect(proof);
            }
            DescriptorEffectTerminal::TxAbort(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .publish_prepared_tx_abort_effect(proof);
            }
        }
    }

    #[inline(never)]
    pub(super) fn rollback_descriptor_effect_terminal_in_core(
        core: &mut ClusterCore<'cfg, T, U, C, MAX_RV>,
        ticket: DescriptorEffectTerminal,
    ) {
        match ticket {
            DescriptorEffectTerminal::StateSnapshot(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .rollback_prepared_state_snapshot_effect(proof);
            }
            DescriptorEffectTerminal::StateRestore(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .rollback_prepared_state_restore_effect(proof);
            }
            DescriptorEffectTerminal::TxCommit(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .rollback_prepared_tx_commit_effect(proof);
            }
            DescriptorEffectTerminal::TxAbort(ticket) => {
                let (owner, proof) = ticket.into_parts();
                core.locals
                    .get_mut_by_proof(owner)
                    .rollback_prepared_tx_abort_effect(proof);
            }
        }
    }
}
