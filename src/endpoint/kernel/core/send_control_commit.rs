use super::{
    BindingSlot, CAP_TOKEN_LEN, ControlOp, CursorEndpoint, DescriptorDispatch, EpochTable,
    LabelUniverse, LoopDecision, LoopRole, RawEmittedCapToken, RouteDecisionSource, ScopeKind,
    SendCommitMeta, SendCommitProof, SendControlDecisionPlan, SendError, SendMeta, SendResult,
    StagedControlEmission, Transport, lane_port,
};
use crate::global::const_dsl::CompactScopeId;

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: crate::runtime::config::Clock,
    E: EpochTable,
    Mint: super::MintConfigMarker,
    B: BindingSlot,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn build_send_control_decision_plan(
        &self,
        meta: SendMeta,
        control: &StagedControlEmission<'_>,
        dispatch: Option<DescriptorDispatch>,
    ) -> SendResult<SendControlDecisionPlan> {
        let Some(dispatch) = dispatch else {
            return Ok(SendControlDecisionPlan::None);
        };
        let is_host_minted = matches!(control, StagedControlEmission::Registered(_));
        if !is_host_minted {
            return Ok(SendControlDecisionPlan::None);
        }
        match dispatch.desc.op() {
            ControlOp::LoopContinue => {
                self.build_loop_control_decision_plan(meta, LoopDecision::Continue, 0)
            }
            ControlOp::LoopBreak => {
                self.build_loop_control_decision_plan(meta, LoopDecision::Break, 1)
            }
            ControlOp::RouteDecision => {
                let arm = meta.route_arm.ok_or(SendError::PhaseInvariant)?;
                if arm > 1 {
                    return Err(SendError::PhaseInvariant);
                }
                Ok(SendControlDecisionPlan::Route {
                    scope: CompactScopeId::from_scope_id(meta.scope),
                    arm,
                    source: RouteDecisionSource::Resolver,
                    lane: meta.lane,
                })
            }
            _ => Ok(SendControlDecisionPlan::None),
        }
    }

    #[inline(never)]
    fn build_loop_control_decision_plan(
        &self,
        meta: SendMeta,
        decision: LoopDecision,
        arm: u8,
    ) -> SendResult<SendControlDecisionPlan> {
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
            return Ok(SendControlDecisionPlan::Loop {
                scope: CompactScopeId::from_scope_id(loop_scope),
                idx,
                decision,
                lane: meta.lane,
            });
        }
        if loop_scope.kind() == ScopeKind::Route {
            return Ok(SendControlDecisionPlan::Route {
                scope: CompactScopeId::from_scope_id(loop_scope),
                arm,
                source: RouteDecisionSource::Ack,
                lane: meta.lane,
            });
        }
        let idx = Self::loop_index(loop_scope).ok_or(SendError::PhaseInvariant)?;
        Ok(SendControlDecisionPlan::Loop {
            scope: CompactScopeId::from_scope_id(loop_scope),
            idx,
            decision,
            lane: meta.lane,
        })
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn publish_send_control_decision_plan(
        &mut self,
        plan: SendControlDecisionPlan,
    ) {
        match plan {
            SendControlDecisionPlan::None => {}
            SendControlDecisionPlan::Route {
                scope,
                arm,
                source,
                lane,
            } => {
                let scope = scope.to_scope_id();
                self.record_route_decision_for_scope_lanes(scope, arm, lane);
                self.emit_route_decision(scope, arm, source, lane);
            }
            SendControlDecisionPlan::Loop {
                scope,
                idx,
                decision,
                lane,
            } => {
                let scope = scope.to_scope_id();
                let port = self.port_for_lane(lane as usize);
                let disposition = match decision {
                    LoopDecision::Continue => crate::rendezvous::tables::LoopDisposition::Continue,
                    LoopDecision::Break => crate::rendezvous::tables::LoopDisposition::Break,
                };
                let arm = match decision {
                    LoopDecision::Continue => 0,
                    LoopDecision::Break => 1,
                };
                let epoch = port.record_loop_decision(idx, disposition);
                let ts = port.now32();
                let causal = crate::observe::core::TapEvent::make_causal_key(ROLE, idx);
                let arg1 = match decision {
                    LoopDecision::Continue => ((idx as u32) << 16) | epoch as u32,
                    LoopDecision::Break => ((idx as u32) << 16) | (epoch as u32) | 0x1,
                };
                let event = crate::observe::events::LoopDecision::with_causal_and_scope(
                    ts,
                    causal,
                    self.sid.raw(),
                    arg1,
                    self.scope_trace(scope).map(|t| t.pack()).unwrap_or(0),
                );
                crate::observe::core::emit(port.tap(), event);
                if scope.kind() == crate::global::const_dsl::ScopeKind::Route {
                    self.record_route_decision_for_scope_lanes(scope, arm, lane);
                    self.emit_route_decision(scope, arm, RouteDecisionSource::Ack, lane);
                }
            }
        }
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel::core) fn finish_send_control_outcome(
        &self,
        meta: SendCommitMeta,
        control: StagedControlEmission<'r>,
    ) {
        match control {
            StagedControlEmission::None => {}
            StagedControlEmission::Registered(rollback) => {
                self.finish_registered_send_control_outcome(meta, rollback)
            }
            StagedControlEmission::WireOnly => self.finish_wire_send_control_outcome(meta),
        }
    }

    #[inline(never)]
    fn finish_registered_send_control_outcome(
        &self,
        meta: SendCommitMeta,
        rollback: super::PendingCapRelease<'r>,
    ) {
        drop(rollback.into_registered_token(self.send_control_token_bytes(meta)));
    }

    #[inline(never)]
    fn finish_wire_send_control_outcome(&self, meta: SendCommitMeta) {
        let _ = RawEmittedCapToken::new(self.send_control_token_bytes(meta)).bytes();
    }

    #[inline(always)]
    fn send_control_token_bytes(&self, meta: SendCommitMeta) -> [u8; CAP_TOKEN_LEN] {
        let port = self.port_for_lane(meta.lane as usize);
        let scratch_ptr = lane_port::scratch_ptr(port);
        let scratch = /* SAFETY: the send commit path holds endpoint ownership until publication finishes, and the outgoing control token was staged into this lane scratch before transport began. */ unsafe { &*scratch_ptr };
        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes.copy_from_slice(&scratch[..CAP_TOKEN_LEN]);
        bytes
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn rollback_send_commit_proof(
        &self,
        proof: Option<SendCommitProof<'r>>,
    ) {
        if let Some(proof) = proof {
            proof.descriptor.rollback();
        }
    }

    #[inline(never)]
    pub(in crate::endpoint::kernel) fn rollback_send_commit_plan(
        &self,
        plan: Option<super::SendCommitPlan<'r>>,
    ) {
        self.rollback_send_commit_proof(plan.map(|plan| plan.proof));
    }
}

#[cfg(test)]
mod tests {
    use super::RawEmittedCapToken;
    use crate::{
        control::cap::{
            mint::{
                CAP_HEADER_LEN, CAP_NONCE_LEN, CAP_TOKEN_LEN, CapHeader, CapShot,
                ControlResourceKind, ResourceKind,
            },
            resource_kinds::{LoopContinueKind, LoopDecisionHandle},
            typed_tokens::RawRegisteredCapToken,
        },
        global::const_dsl::ScopeId,
        integration::ids::{Lane, SessionId},
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

    fn make_test_token_bytes(
        nonce: [u8; CAP_NONCE_LEN],
        handle: &LoopDecisionHandle,
    ) -> [u8; CAP_TOKEN_LEN] {
        let handle_bytes = LoopContinueKind::encode_handle(handle);
        let mut header = [0u8; CAP_HEADER_LEN];
        CapHeader::new(
            SessionId::new(handle.sid),
            Lane::new(handle.lane as u32),
            0,
            LoopContinueKind::TAG,
            LoopContinueKind::OP,
            LoopContinueKind::PATH,
            CapShot::Many,
            LoopContinueKind::SCOPE,
            0,
            handle.scope.local_ordinal(),
            0,
            handle_bytes,
        )
        .encode(&mut header);

        let mut bytes = [0u8; CAP_TOKEN_LEN];
        bytes[..CAP_NONCE_LEN].copy_from_slice(&nonce);
        bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
        bytes
    }

    #[test]
    fn registered_send_control_outcome_releases_token_on_finish() {
        let table = cap_table();
        let lane = Lane::new(3);
        let sid = SessionId::new(42);
        let nonce = [0xAC; CAP_NONCE_LEN];
        let handle = LoopDecisionHandle {
            sid: sid.raw(),
            lane: lane.as_wire(),
            scope: ScopeId::loop_scope(2),
        };
        let bytes = make_test_token_bytes(nonce, &handle);

        table
            .insert_entry(CapEntry::new(lane, 1, nonce))
            .expect("insert succeeds");

        let mut snapshot_storage = vec![0u8; StateSnapshotTable::storage_bytes(1)];
        let mut snapshots = StateSnapshotTable::empty();
        unsafe {
            // SAFETY: snapshot_storage is uniquely owned by this test and sized
            // for one lane entry starting at the tested lane.
            snapshots.bind_from_storage(snapshot_storage.as_mut_ptr(), lane.raw(), 1);
        }
        let revisions = Cell::new(0u64);

        drop(RawRegisteredCapToken::from_registered_bytes(
            bytes,
            nonce,
            CapReleaseCtx::new(&table, &snapshots, &revisions, lane),
        ));

        assert!(
            !table.release_by_nonce(&nonce),
            "finishing a registered send must release the registered capability"
        );
    }

    #[test]
    fn emitted_send_control_outcome_finishes_without_registration() {
        let bytes = [0u8; CAP_TOKEN_LEN];
        let _ = RawEmittedCapToken::new(bytes).bytes();
    }
}
