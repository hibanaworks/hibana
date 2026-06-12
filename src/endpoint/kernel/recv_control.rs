//! Inbound wire-control validation shared by recv and route decode.

use super::{core::CursorEndpoint, recv::RecvDescriptor};
use crate::{
    control::{
        cap::mint::{CAP_TOKEN_LEN, ControlOp, ControlPath, EpochTable, MintConfigMarker},
        cluster::core::{SessionCluster, TopologyDescriptor},
        types::Lane,
    },
    endpoint::{RecvError, RecvResult},
    global::ControlDesc,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::Payload},
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel) fn validate_inbound_wire_control(
        &self,
        desc: RecvDescriptor,
        control: Option<ControlDesc>,
        payload: Payload<'_>,
    ) -> RecvResult<()> {
        let Some(control) = control else {
            return Ok(());
        };
        if !matches!(control.path(), ControlPath::Wire) {
            return Ok(());
        }
        if !desc.meta.is_control {
            return Err(RecvError::PhaseInvariant);
        }
        let bytes = payload.as_bytes();
        if bytes.len() != CAP_TOKEN_LEN {
            return Err(RecvError::PhaseInvariant);
        }
        let mut token_bytes = [0u8; CAP_TOKEN_LEN];
        token_bytes.copy_from_slice(bytes);
        let lane = Lane::new(desc.lane_wire as u32);
        let epoch = self.descriptor_recv_epoch(control, lane)?;
        let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
        if matches!(
            control.op(),
            ControlOp::TopologyBegin | ControlOp::TopologyAck | ControlOp::TopologyCommit
        ) {
            return self.validate_inbound_topology_wire_control(
                desc,
                control,
                token_bytes,
                lane,
                epoch,
                cluster,
            );
        }
        cluster
            .validate_bound_descriptor_control_frame(
                self.rendezvous_id(),
                token_bytes,
                control,
                self.sid,
                lane,
                ROLE,
                desc.meta.scope.local_ordinal(),
                epoch,
            )
            .map(|_| ())
            .map_err(|_| RecvError::PhaseInvariant)
    }

    #[inline(never)]
    fn validate_inbound_topology_wire_control(
        &self,
        desc: RecvDescriptor,
        control: ControlDesc,
        token_bytes: [u8; CAP_TOKEN_LEN],
        lane: Lane,
        epoch: u16,
        _cluster: &SessionCluster<'r, T, U, C, MAX_RV>,
    ) -> RecvResult<()> {
        let token = crate::control::cap::mint::ControlToken::from_raw_bytes(token_bytes);
        let header = token
            .control_header()
            .map_err(|_| RecvError::PhaseInvariant)?;
        SessionCluster::<T, U, C, MAX_RV>::verify_control_header(
            control,
            header,
            desc.meta.scope.local_ordinal(),
            epoch,
        )
        .map_err(|_| RecvError::PhaseInvariant)?;
        if header.sid() != self.sid || header.lane() != lane || header.role() != ROLE {
            return Err(RecvError::PhaseInvariant);
        }

        let operands = TopologyDescriptor::decode_for(control.op(), token.handle_bytes())
            .map_err(|_| RecvError::PhaseInvariant)?
            .operands();
        match control.op() {
            ControlOp::TopologyBegin | ControlOp::TopologyCommit => {
                if operands.dst_rv != self.rendezvous_id() {
                    return Err(RecvError::PhaseInvariant);
                }
            }
            ControlOp::TopologyAck => {
                if operands.src_rv != self.rendezvous_id() || operands.src_lane != lane {
                    return Err(RecvError::PhaseInvariant);
                }
            }
            _ => return Err(RecvError::PhaseInvariant),
        }
        Ok(())
    }

    #[inline]
    fn descriptor_recv_epoch(&self, control: ControlDesc, lane: Lane) -> RecvResult<u16> {
        match control.op() {
            ControlOp::StateSnapshot => {
                let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(RecvError::PhaseInvariant)?;
                Ok(rendezvous.lane_generation(lane).raw())
            }
            ControlOp::StateRestore | ControlOp::TxCommit | ControlOp::TxAbort => {
                let cluster = self.control.cluster().ok_or(RecvError::PhaseInvariant)?;
                let rendezvous = cluster
                    .get_local(&self.rendezvous_id())
                    .ok_or(RecvError::PhaseInvariant)?;
                rendezvous
                    .snapshot_generation(lane)
                    .map(|generation| generation.raw())
                    .ok_or(RecvError::PhaseInvariant)
            }
            ControlOp::LoopContinue
            | ControlOp::LoopBreak
            | ControlOp::TopologyBegin
            | ControlOp::TopologyAck
            | ControlOp::TopologyCommit => Ok(0),
        }
    }
}
