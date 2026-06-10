use super::{
    ControlOp, CursorEndpoint, DescriptorDispatch, EpochTable, LabelUniverse, LoopCommitRow,
    LoopDecision, LoopRole, SendError, SendMeta, SendResult, StagedControlEmission, Transport,
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: super::MintConfigMarker,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn build_send_loop_commit_row(
        &self,
        meta: SendMeta,
        control: &StagedControlEmission<'_>,
        dispatch: Option<DescriptorDispatch>,
    ) -> SendResult<LoopCommitRow> {
        let Some(dispatch) = dispatch else {
            return Ok(LoopCommitRow::EMPTY);
        };
        let is_host_minted = matches!(control, StagedControlEmission::Registered(_));
        if !is_host_minted {
            return Ok(LoopCommitRow::EMPTY);
        }
        match dispatch.desc.op() {
            ControlOp::LoopContinue => {
                self.build_loop_control_commit_row(meta, LoopDecision::Continue)
            }
            ControlOp::LoopBreak => self.build_loop_control_commit_row(meta, LoopDecision::Break),
            ControlOp::StateSnapshot
            | ControlOp::StateRestore
            | ControlOp::TopologyBegin
            | ControlOp::TopologyAck
            | ControlOp::TopologyCommit
            | ControlOp::AbortBegin
            | ControlOp::AbortAck
            | ControlOp::Fence
            | ControlOp::TxCommit
            | ControlOp::TxAbort => Ok(LoopCommitRow::EMPTY),
        }
    }

    #[inline(never)]
    fn build_loop_control_commit_row(
        &self,
        meta: SendMeta,
        decision: LoopDecision,
    ) -> SendResult<LoopCommitRow> {
        let loop_scope = if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            metadata.scope
        } else {
            meta.scope
        };
        if loop_scope.is_none() {
            return Err(SendError::PhaseInvariant);
        }
        if let Some(metadata) = self.cursor.loop_metadata_inner()
            && metadata.role == LoopRole::Controller
            && metadata.controller == ROLE
        {
            let idx = Self::loop_index(loop_scope).ok_or(SendError::PhaseInvariant)?;
            return Ok(LoopCommitRow::decision(
                loop_scope, idx, meta.lane, decision,
            ));
        }
        let idx = Self::loop_index(loop_scope).ok_or(SendError::PhaseInvariant)?;
        Ok(LoopCommitRow::decision(
            loop_scope, idx, meta.lane, decision,
        ))
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn finish_send_control_outcome(
        &self,
        control: StagedControlEmission<'r>,
    ) {
        match control {
            StagedControlEmission::None => {}
            StagedControlEmission::Registered(release) => {
                self.finish_registered_send_control_outcome(release)
            }
            StagedControlEmission::WireOnly => {}
        }
    }

    #[inline(never)]
    fn finish_registered_send_control_outcome(&self, release: super::PendingCapRelease<'r>) {
        release.release_now();
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn rollback_send_commit_plan(
        &self,
        plan: Option<super::SendCommitPlan<'r>>,
    ) {
        if let Some(plan) = plan {
            let (control, descriptor) = plan.into_rollback_parts();
            self.rollback_send_descriptor_terminal(descriptor);
            drop(control);
        }
    }

    #[inline(never)]
    fn rollback_send_descriptor_terminal(&self, terminal: super::SendDescriptorTerminal<'r>) {
        let Some(ticket) = terminal.into_ticket() else {
            return;
        };
        let cluster = self
            .control
            .cluster()
            .expect("send descriptor rollback requires its preparing cluster");
        cluster.rollback_descriptor_terminal(ticket);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        control::cap::mint::CAP_NONCE_LEN,
        control::types::Lane,
        rendezvous::{
            capability::{CapEntry, CapReleaseCtx, CapTable},
            tables::StateSnapshotTable,
        },
    };
    use core::cell::Cell;
    use std::vec;

    fn cap_table() -> CapTable {
        const CAP_TABLE_SLOTS: usize = 64;
        let mut table = CapTable::empty();
        let storage = vec![Option::<CapEntry>::None; CAP_TABLE_SLOTS].into_boxed_slice();
        let ptr = std::boxed::Box::leak(storage).as_mut_ptr().cast::<u8>();
        unsafe {
            // SAFETY: the leaked test storage is writable for the duration of
            // the table and is sized for exactly CAP_TABLE_SLOTS entries.
            table.bind_from_storage(ptr, CAP_TABLE_SLOTS, 0);
        }
        table
    }

    #[test]
    fn registered_send_control_outcome_releases_token_on_finish() {
        let table = cap_table();
        let lane = Lane::new(3);
        let nonce = [0xAC; CAP_NONCE_LEN];

        table
            .insert_entry_with(|| CapEntry::new(lane, 1, nonce))
            .expect("insert succeeds");

        let mut snapshot_storage = vec![0u8; StateSnapshotTable::storage_bytes(1)];
        let mut snapshots = StateSnapshotTable::empty();
        unsafe {
            // SAFETY: snapshot_storage is uniquely owned by this test and sized
            // for one lane entry starting at the tested lane.
            snapshots.bind_from_storage(snapshot_storage.as_mut_ptr(), lane.raw(), 1);
        }
        let revisions = Cell::new(0u64);

        super::super::PendingCapRelease::new(
            nonce,
            CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
        )
        .release_now();

        assert!(
            !table.release_by_nonce(&nonce),
            "finishing a registered send must release the registered capability"
        );
    }
}
