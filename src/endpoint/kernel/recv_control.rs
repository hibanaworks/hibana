//! Inbound explicit-control validation shared by recv and route decode.

use super::{core::CursorEndpoint, recv::RecvDescriptor};
use crate::{
    binding::EndpointSlot,
    control::{
        cap::mint::{CAP_TOKEN_LEN, ControlOp, ControlPath, EpochTable, MintConfigMarker},
        types::Lane,
    },
    endpoint::{RecvError, RecvResult},
    global::ControlDesc,
    runtime::{config::Clock, consts::LabelUniverse},
    transport::{Transport, wire::Payload},
};

impl<'r, const ROLE: u8, T, U, C, E, const MAX_RV: usize, Mint, B>
    CursorEndpoint<'r, ROLE, T, U, C, E, MAX_RV, Mint, B>
where
    T: Transport + 'r,
    U: LabelUniverse,
    C: Clock,
    E: EpochTable,
    Mint: MintConfigMarker,
    B: EndpointSlot + 'r,
{
    #[inline(never)]
    pub(in crate::endpoint::kernel) fn validate_inbound_explicit_wire_control(
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

    #[inline]
    fn descriptor_recv_epoch(&self, control: ControlDesc, lane: Lane) -> RecvResult<u16> {
        match control.op() {
            ControlOp::AbortAck | ControlOp::StateSnapshot => {
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
            _ => Ok(0),
        }
    }
}
