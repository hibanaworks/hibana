use crate::control::cap::mint::{CAP_HANDLE_LEN, CapError};
#[cfg(test)]
use crate::control::types::{Lane, SessionId};

/// Flags stored inside [`TopologyHandle::flags`].
pub(crate) mod topology_flags {
    /// Indicates that `seq_tx` / `seq_rx` contain fence counters.
    pub(crate) const FENCES_PRESENT: u16 = 0x0001;
}

/// Handle payload for topology-control operations.
///
/// Encoding layout (big-endian):
/// ```text
/// [ 0..2 )  src_rv
/// [ 2..4 )  dst_rv
/// [ 4..6 )  src_lane
/// [ 6..8 )  dst_lane
/// [ 8..10)  old_gen
/// [10..12)  new_gen
/// [12..16)  seq_tx
/// [16..20)  seq_rx
/// [20..22)  flags (see [`topology_flags`])
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct TopologyHandle {
    pub src_rv: u16,
    pub dst_rv: u16,
    pub src_lane: u16,
    pub dst_lane: u16,
    pub old_gen: u16,
    pub new_gen: u16,
    pub seq_tx: u32,
    pub seq_rx: u32,
    pub flags: u16,
}

impl TopologyHandle {
    pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..2].copy_from_slice(&self.src_rv.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_rv.to_be_bytes());
        buf[4..6].copy_from_slice(&self.src_lane.to_be_bytes());
        buf[6..8].copy_from_slice(&self.dst_lane.to_be_bytes());
        buf[8..10].copy_from_slice(&self.old_gen.to_be_bytes());
        buf[10..12].copy_from_slice(&self.new_gen.to_be_bytes());
        buf[12..16].copy_from_slice(&self.seq_tx.to_be_bytes());
        buf[16..20].copy_from_slice(&self.seq_rx.to_be_bytes());
        buf[20..22].copy_from_slice(&self.flags.to_be_bytes());
        buf
    }

    pub(crate) fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        Ok(Self {
            src_rv: u16::from_be_bytes([data[0], data[1]]),
            dst_rv: u16::from_be_bytes([data[2], data[3]]),
            src_lane: u16::from_be_bytes([data[4], data[5]]),
            dst_lane: u16::from_be_bytes([data[6], data[7]]),
            old_gen: u16::from_be_bytes([data[8], data[9]]),
            new_gen: u16::from_be_bytes([data[10], data[11]]),
            seq_tx: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            seq_rx: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            flags: u16::from_be_bytes([data[20], data[21]]),
        })
    }
}

/// Handle payload for delegation operations.
///
/// Encoding layout (big-endian):
/// ```text
/// [ 0..2 )  src_rv
/// [ 2..4 )  dst_rv
/// [ 4..6 )  src_lane
/// [ 6..8 )  dst_lane
/// [ 8..12)  seq_tx
/// [12..16)  seq_rx
/// [16..20)  shard / policy metadata
/// [20..22)  flags
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct DelegationHandle {
    pub src_rv: u16,
    pub dst_rv: u16,
    pub src_lane: u16,
    pub dst_lane: u16,
    pub seq_tx: u32,
    pub seq_rx: u32,
    pub shard: u32,
    pub flags: u16,
}

impl DelegationHandle {
    pub(crate) fn encode(self) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0..2].copy_from_slice(&self.src_rv.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_rv.to_be_bytes());
        buf[4..6].copy_from_slice(&self.src_lane.to_be_bytes());
        buf[6..8].copy_from_slice(&self.dst_lane.to_be_bytes());
        buf[8..12].copy_from_slice(&self.seq_tx.to_be_bytes());
        buf[12..16].copy_from_slice(&self.seq_rx.to_be_bytes());
        buf[16..20].copy_from_slice(&self.shard.to_be_bytes());
        buf[20..22].copy_from_slice(&self.flags.to_be_bytes());
        buf
    }
}

pub(crate) type SessionLaneHandle = (u32, u16);

#[cfg(test)]
pub(crate) const TAG_STATE_SNAPSHOT_CONTROL: u8 = 0x42;
#[cfg(test)]
pub(crate) const TAG_ABORT_BEGIN_CONTROL: u8 = 0x45;
#[cfg(test)]
pub(crate) const TAG_CAP_DELEGATE_CONTROL: u8 = 0x49;
#[cfg(test)]
pub(crate) const TAG_TOPOLOGY_BEGIN_CONTROL: u8 = 0x57;

#[cfg(test)]
#[inline]
pub(crate) fn encode_session_lane_handle(handle: SessionLaneHandle) -> [u8; CAP_HANDLE_LEN] {
    let mut buf = [0u8; CAP_HANDLE_LEN];
    buf[0..4].copy_from_slice(&handle.0.to_le_bytes());
    buf[4..6].copy_from_slice(&handle.1.to_le_bytes());
    buf
}

#[inline]
pub(crate) fn decode_session_lane_handle(
    data: [u8; CAP_HANDLE_LEN],
) -> Result<SessionLaneHandle, CapError> {
    let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let lane = u16::from_le_bytes([data[4], data[5]]);
    Ok((sid, lane))
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn mint_session_lane_handle(sid: SessionId, lane: Lane) -> SessionLaneHandle {
    (sid.raw(), lane.raw() as u16)
}
