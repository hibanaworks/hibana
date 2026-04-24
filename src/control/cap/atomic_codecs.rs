use crate::control::cap::mint::{CAP_HANDLE_LEN, CapError};
#[cfg(test)]
use crate::control::types::{Lane, SessionId};

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
/// [20..22)  reserved, must be zero
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
        buf
    }

    pub(crate) fn decode(data: [u8; CAP_HANDLE_LEN]) -> Result<Self, CapError> {
        let flags = u16::from_be_bytes([data[20], data[21]]);
        if flags != 0 {
            return Err(CapError::Mismatch);
        }
        Ok(Self {
            src_rv: u16::from_be_bytes([data[0], data[1]]),
            dst_rv: u16::from_be_bytes([data[2], data[3]]),
            src_lane: u16::from_be_bytes([data[4], data[5]]),
            dst_lane: u16::from_be_bytes([data[6], data[7]]),
            old_gen: u16::from_be_bytes([data[8], data[9]]),
            new_gen: u16::from_be_bytes([data[10], data[11]]),
            seq_tx: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            seq_rx: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
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
    if data[6..].iter().any(|byte| *byte != 0) {
        return Err(CapError::Mismatch);
    }
    let sid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let lane = u16::from_le_bytes([data[4], data[5]]);
    Ok((sid, lane))
}

#[cfg(test)]
#[inline(always)]
pub(crate) const fn mint_session_lane_handle(sid: SessionId, lane: Lane) -> SessionLaneHandle {
    (sid.raw(), lane.raw() as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_handle_rejects_reserved_flags() {
        let handle = TopologyHandle {
            src_rv: 1,
            dst_rv: 2,
            src_lane: 3,
            dst_lane: 4,
            old_gen: 5,
            new_gen: 6,
            seq_tx: 7,
            seq_rx: 8,
        };
        let encoded = handle.encode();
        assert_eq!(TopologyHandle::decode(encoded), Ok(handle));

        let mut flagged = encoded;
        flagged[21] = 1;
        assert_eq!(TopologyHandle::decode(flagged), Err(CapError::Mismatch));
    }

    #[test]
    fn topology_handle_has_no_flags_field() {
        let source = include_str!("atomic_codecs.rs");
        let start = source.find("struct TopologyHandle").unwrap();
        let end = source.find("impl TopologyHandle").unwrap();
        assert!(
            !source[start..end].contains("flags:"),
            "topology handle must keep [20..22) as reserved wire bytes, not a runtime field"
        );
    }

    #[test]
    fn session_lane_handle_rejects_reserved_tail() {
        let handle = mint_session_lane_handle(SessionId::new(11), Lane::new(3));
        let encoded = encode_session_lane_handle(handle);
        assert_eq!(decode_session_lane_handle(encoded), Ok(handle));

        let mut trailing = encoded;
        trailing[6] = 0xA5;
        assert_eq!(
            decode_session_lane_handle(trailing),
            Err(CapError::Mismatch)
        );
    }
}
