use super::commit_delta::PreparedCommitDelta;
use super::{
    Lane, LaneGuard, Port, ResidentActor, SendResult, SendRouteAudit, Transport, lane_port,
};

pub(in crate::endpoint::kernel::core) struct SendProgressCommitPlan {
    pub(crate) delta: PreparedCommitDelta,
    pub(crate) route_audit: SendRouteAudit,
}

mod commit;
pub(crate) use commit::{CommitDelta, CommitEventRow, CommitRow};

pub(crate) struct SendCommitProof<'rv> {
    pub(in crate::endpoint::kernel::core) transfer:
        Option<crate::endpoint::transfer::EndpointTransfer>,
    pub(in crate::endpoint::kernel::core) progress: SendProgressCommitPlan,
    pub(in crate::endpoint::kernel::core) _borrow: core::marker::PhantomData<&'rv ()>,
}

pub(crate) struct SendCommitPlan<'rv> {
    pub(in crate::endpoint::kernel::core) proof: SendCommitProof<'rv>,
}

pub(crate) struct PendingSendIo<'r> {
    pub(in crate::endpoint::kernel) lane: Lane,
    pub(in crate::endpoint::kernel) transport: lane_port::PendingSend,
    pub(in crate::endpoint::kernel) commit_plan: Option<SendCommitPlan<'r>>,
}

impl<'r> PendingSendIo<'r> {
    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_idx(&self) -> usize {
        self.lane.raw() as usize
    }

    #[inline(always)]
    pub(in crate::endpoint::kernel) fn lane_wire(&self) -> u8 {
        self.lane.as_wire()
    }
}

pub(crate) enum SendInitOutcome<'r> {
    Ready(SendResult<SendCommitOutcome<'r>>),
    Pending { pending: PendingSendIo<'r> },
}

pub(crate) struct SendCommitOutcome<'rv> {
    pub(crate) _borrow: core::marker::PhantomData<&'rv ()>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct MsgCore {
    logical_label: crate::transport::LogicalLabel,
    frame_label: crate::transport::FrameLabel,
}

impl MsgCore {
    #[inline]
    pub(crate) const fn new(logical_label: u8, frame_label: crate::transport::FrameLabel) -> Self {
        Self {
            logical_label: crate::transport::LogicalLabel::new(logical_label),
            frame_label,
        }
    }

    #[inline]
    pub(crate) const fn logical_label(self) -> u8 {
        self.logical_label.raw()
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.frame_label
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SendRuntimeDesc {
    pub(crate) core: MsgCore,
}

impl SendRuntimeDesc {
    #[inline]
    pub(crate) const fn new(logical_label: u8, frame_label: crate::transport::FrameLabel) -> Self {
        Self {
            core: MsgCore::new(logical_label, frame_label),
        }
    }

    #[inline]
    pub(crate) const fn logical_label(self) -> u8 {
        self.core.logical_label()
    }

    #[inline]
    pub(crate) const fn frame_label(self) -> crate::transport::FrameLabel {
        self.core.frame_label()
    }
}

pub(crate) enum SendState<'r> {
    Preparing {
        logical_label: u8,
        payload_kind: crate::eff::EventPayload,
        input: Option<lane_port::SendInput>,
    },
    Sending {
        pending: PendingSendIo<'r>,
    },
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActorStorageLayout {
    header_bytes: usize,
    header_align: usize,
    views_offset: usize,
    port_slots_offset: usize,
    port_slots_bytes: usize,
    port_slots_align: usize,
    guard_slots_offset: usize,
    guard_slots_bytes: usize,
    guard_slots_align: usize,
    arena_offset: usize,
    arena_bytes: usize,
    arena_align: usize,
    total_bytes: usize,
    total_align: usize,
}

impl ActorStorageLayout {
    pub(crate) const fn views_offset(self) -> usize {
        self.views_offset
    }
    #[inline(always)]
    pub(crate) const fn port_slots_offset(self) -> usize {
        self.port_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn guard_slots_offset(self) -> usize {
        self.guard_slots_offset
    }

    #[inline(always)]
    pub(crate) const fn arena_offset(self) -> usize {
        self.arena_offset
    }

    #[inline(always)]
    pub(crate) const fn total_bytes(self) -> usize {
        self.total_bytes
    }

    #[inline(always)]
    pub(crate) const fn total_align(self) -> usize {
        self.total_align
    }
}

#[inline(always)]
const fn storage_max(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[inline]
pub(crate) fn actor_storage_layout<'r, T>(
    descriptor: crate::global::compiled::images::RoleDescriptorRef,
) -> ActorStorageLayout
where
    T: Transport + 'r,
{
    let arena_layout = descriptor.endpoint_arena_layout();
    let lane_slot_count = descriptor.all_view_lane_slots();
    let header_bytes = core::mem::size_of::<ResidentActor<'r, T>>();
    let header_align = core::mem::align_of::<ResidentActor<'r, T>>();
    let view_align = core::mem::align_of::<super::ResidentView<'r, T>>();
    let views_offset = crate::runtime_core::layout::align_up(header_bytes, view_align);
    let views_bytes = crate::runtime_core::layout::mul(
        core::mem::size_of::<super::ResidentView<'r, T>>(),
        descriptor.logical_views().len(),
    );
    let port_slots_align = core::mem::align_of::<Option<Port<'r, T>>>();
    let port_slots_bytes = crate::runtime_core::layout::mul(
        core::mem::size_of::<Option<Port<'r, T>>>(),
        lane_slot_count,
    );
    let port_slots_offset = crate::runtime_core::layout::align_up(
        crate::runtime_core::layout::add(views_offset, views_bytes),
        port_slots_align,
    );
    let guard_slots_align = core::mem::align_of::<Option<LaneGuard<'r, T>>>();
    let guard_slots_bytes = crate::runtime_core::layout::mul(
        core::mem::size_of::<Option<LaneGuard<'r, T>>>(),
        lane_slot_count,
    );
    let guard_slots_offset = crate::runtime_core::layout::align_up(
        crate::runtime_core::layout::add(port_slots_offset, port_slots_bytes),
        guard_slots_align,
    );
    let arena_offset = crate::runtime_core::layout::align_up(
        crate::runtime_core::layout::add(guard_slots_offset, guard_slots_bytes),
        arena_layout.header_align(),
    );
    let total_align = storage_max(
        storage_max(
            storage_max(storage_max(header_align, view_align), port_slots_align),
            guard_slots_align,
        ),
        arena_layout.header_align(),
    );
    ActorStorageLayout {
        header_bytes,
        header_align,
        views_offset,
        port_slots_offset,
        port_slots_bytes,
        port_slots_align,
        guard_slots_offset,
        guard_slots_bytes,
        guard_slots_align,
        arena_offset,
        arena_bytes: arena_layout.total_bytes(),
        arena_align: arena_layout.total_align(),
        total_bytes: crate::runtime_core::layout::add(arena_offset, arena_layout.total_bytes()),
        total_align,
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn pending_send_commit_proof_stays_compact() {
        assert!(
            core::mem::size_of::<SendCommitProof<'static>>() <= 96,
            "SendCommitProof grew to {} bytes",
            core::mem::size_of::<SendCommitProof<'static>>()
        );
        assert!(
            core::mem::size_of::<SendCommitOutcome<'static>>() <= 56,
            "SendCommitOutcome grew to {} bytes",
            core::mem::size_of::<SendCommitOutcome<'static>>()
        );
        assert!(
            core::mem::size_of::<PendingSendIo<'static>>() <= 184,
            "PendingSendIo grew to {} bytes",
            core::mem::size_of::<PendingSendIo<'static>>()
        );
    }
}
