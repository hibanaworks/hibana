use hibana::{
    g::{self, Msg, Role},
    substrate::{
        cap::{
            CapShot, ControlResourceKind, GenericCapToken, ResourceKind,
            advanced::{
                CAP_HANDLE_LEN, CapError, ControlOp, ControlPath, ControlScopeKind, ScopeId,
            },
        },
        ids::{Lane, SessionId},
        program::{RoleProgram, project},
        wire::{CodecError, Payload, WireEncode, WirePayload},
    },
};

use super::localside;

const WASIP2_STDOUT_LOGICAL: u8 = 17;
const WASIP2_STDOUT_RET_LOGICAL: u8 = 18;
const MEM_BORROW_READ_LOGICAL: u8 = 28;
const MEM_RELEASE_LOGICAL: u8 = 31;
const MEM_GRANT_READ_CONTROL_LOGICAL: u8 = 106;

const TAG_REQ_WASIP2_STDOUT: u8 = 3;
const TAG_RET_WASIP2_STDOUT_WRITTEN: u8 = 3;
const MEM_LEASE_NONE: u8 = 0;
const WASIP2_STREAM_CHUNK_CAPACITY: usize = 30;
const TEST_MEMORY_EPOCH: u32 = 1;
const TEST_STDOUT_PTR: u32 = 1024;
const TEST_STDOUT_BYTES: &[u8] = b"pico memory matrix\n";

#[cfg(feature = "memory-control-1")]
pub const MATRIX_COUNT: usize = 1;
#[cfg(feature = "memory-control-4")]
pub const MATRIX_COUNT: usize = 4;
#[cfg(feature = "memory-control-8")]
pub const MATRIX_COUNT: usize = 8;

pub const ROUTE_SCOPE_COUNT: usize = 0;
pub const EXPECTED_WORKER_BRANCH_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];
pub const ACK_LABELS: [u8; ROUTE_SCOPE_COUNT] = [];

type MemoryLeaseWireHandle = (u8, u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemRights {
    Read,
}

impl MemRights {
    const fn tag(self) -> u8 {
        match self {
            Self::Read => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemReadLeaseKind;

impl ResourceKind for MemReadLeaseKind {
    type Handle = MemoryLeaseWireHandle;
    const TAG: u8 = 0x31;
    const NAME: &'static str = "MemReadLease";

    fn encode_handle(handle: &Self::Handle) -> [u8; CAP_HANDLE_LEN] {
        let mut buf = [0u8; CAP_HANDLE_LEN];
        buf[0] = handle.0;
        buf[1..9].copy_from_slice(&handle.1.to_le_bytes());
        buf
    }

    fn decode_handle(data: [u8; CAP_HANDLE_LEN]) -> Result<Self::Handle, CapError> {
        let mut lease_bytes = [0u8; 8];
        lease_bytes.copy_from_slice(&data[1..9]);
        Ok((data[0], u64::from_le_bytes(lease_bytes)))
    }

    fn zeroize(handle: &mut Self::Handle) {
        *handle = (0, 0);
    }
}

impl ControlResourceKind for MemReadLeaseKind {
    const SCOPE: ControlScopeKind = ControlScopeKind::Policy;
    const TAP_ID: u16 = 0x04d0;
    const SHOT: CapShot = CapShot::One;
    const PATH: ControlPath = ControlPath::Wire;
    const OP: ControlOp = ControlOp::Fence;
    const AUTO_MINT_WIRE: bool = true;

    fn mint_handle(_session: SessionId, _lane: Lane, _scope: ScopeId) -> Self::Handle {
        (MemRights::Read.tag(), 1)
    }
}

type MemReadGrantControl =
    Msg<MEM_GRANT_READ_CONTROL_LOGICAL, GenericCapToken<MemReadLeaseKind>, MemReadLeaseKind>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemBorrow {
    ptr: u32,
    len: u8,
    epoch: u32,
}

impl MemBorrow {
    const fn new(ptr: u32, len: u8, epoch: u32) -> Self {
        Self { ptr, len, epoch }
    }

    const fn ptr(&self) -> u32 {
        self.ptr
    }

    const fn len(&self) -> u8 {
        self.len
    }

    const fn epoch(&self) -> u32 {
        self.epoch
    }
}

impl WireEncode for MemBorrow {
    fn encoded_len(&self) -> Option<usize> {
        Some(9)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 9 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.ptr.to_be_bytes());
        out[4] = self.len;
        out[5..9].copy_from_slice(&self.epoch.to_be_bytes());
        Ok(9)
    }
}

impl WirePayload for MemBorrow {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 9 {
            return Err(CodecError::Invalid("memory borrow carries nine bytes"));
        }
        let mut ptr = [0u8; 4];
        let mut epoch = [0u8; 4];
        ptr.copy_from_slice(&bytes[..4]);
        epoch.copy_from_slice(&bytes[5..9]);
        Ok(Self::new(
            u32::from_be_bytes(ptr),
            bytes[4],
            u32::from_be_bytes(epoch),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemGrant {
    lease_id: u8,
    ptr: u32,
    len: u8,
    epoch: u32,
    rights: MemRights,
}

impl MemGrant {
    const fn new(lease_id: u8, ptr: u32, len: u8, epoch: u32, rights: MemRights) -> Self {
        Self {
            lease_id,
            ptr,
            len,
            epoch,
            rights,
        }
    }

    const fn lease_id(&self) -> u8 {
        self.lease_id
    }

    const fn len(&self) -> u8 {
        self.len
    }

    const fn rights(&self) -> MemRights {
        self.rights
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemRelease {
    lease_id: u8,
}

impl MemRelease {
    const fn new(lease_id: u8) -> Self {
        Self { lease_id }
    }

    const fn lease_id(&self) -> u8 {
        self.lease_id
    }
}

impl WireEncode for MemRelease {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.lease_id;
        Ok(1)
    }
}

impl WirePayload for MemRelease {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 1 {
            return Err(CodecError::Invalid("memory release carries one byte"));
        }
        Ok(Self::new(bytes[0]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wasip2StreamChunk {
    lease_id: u8,
    len: u8,
    bytes: [u8; WASIP2_STREAM_CHUNK_CAPACITY],
}

type StdoutChunk = Wasip2StreamChunk;

impl Wasip2StreamChunk {
    fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::new_with_lease(MEM_LEASE_NONE, bytes)
    }

    fn new_with_lease(lease_id: u8, bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > WASIP2_STREAM_CHUNK_CAPACITY {
            return Err(CodecError::Invalid("stream chunk exceeds fixed capacity"));
        }
        let mut out = [0u8; WASIP2_STREAM_CHUNK_CAPACITY];
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            lease_id,
            len: bytes.len() as u8,
            bytes: out,
        })
    }

    fn with_lease(&self, lease_id: u8) -> Self {
        Self {
            lease_id,
            len: self.len,
            bytes: self.bytes,
        }
    }

    const fn lease_id(&self) -> u8 {
        self.lease_id
    }

    const fn len(&self) -> usize {
        self.len as usize
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len()]
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 2 {
            return Err(CodecError::Truncated);
        }
        let lease_id = bytes[0];
        let len = bytes[1] as usize;
        let payload = &bytes[2..];
        if payload.len() != len {
            return Err(CodecError::Invalid("stream chunk length mismatch"));
        }
        Self::new_with_lease(lease_id, payload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineReq {
    Wasip2Stdout(StdoutChunk),
}

impl WireEncode for EngineReq {
    fn encoded_len(&self) -> Option<usize> {
        Some(match self {
            Self::Wasip2Stdout(chunk) => 3 + chunk.len(),
        })
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        match *self {
            Self::Wasip2Stdout(chunk) => {
                let len = chunk.len();
                if out.len() < 3 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASIP2_STDOUT;
                out[1] = chunk.lease_id();
                out[2] = len as u8;
                out[3..3 + len].copy_from_slice(chunk.as_bytes());
                Ok(3 + len)
            }
        }
    }
}

impl WirePayload for EngineReq {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        let Some((&tag, rest)) = bytes.split_first() else {
            return Err(CodecError::Truncated);
        };
        match tag {
            TAG_REQ_WASIP2_STDOUT => Ok(Self::Wasip2Stdout(StdoutChunk::decode(rest)?)),
            _ => Err(CodecError::Invalid("unknown engine request tag")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EngineRet {
    Wasip2StdoutWritten(u8),
}

impl WireEncode for EngineRet {
    fn encoded_len(&self) -> Option<usize> {
        Some(2)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        match *self {
            Self::Wasip2StdoutWritten(written) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASIP2_STDOUT_WRITTEN;
                out[1] = written;
                Ok(2)
            }
        }
    }
}

impl WirePayload for EngineRet {
    type Decoded<'a> = Self;

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 2 {
            return Err(CodecError::Invalid("stdout reply carries two bytes"));
        }
        if bytes[0] != TAG_RET_WASIP2_STDOUT_WRITTEN {
            return Err(CodecError::Invalid("unknown engine reply tag"));
        }
        Ok(Self::Wasip2StdoutWritten(bytes[1]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryLeaseError {
    Empty,
    TooLarge,
    EpochMismatch,
    OutOfBounds,
    TableFull,
    InvalidLeaseId,
    UnknownLease,
    RightsMismatch,
    LengthExceeded,
}

struct MemoryLeaseTable<const N: usize> {
    memory_len: u32,
    epoch: u32,
    slots: [Option<MemGrant>; N],
}

impl<const N: usize> MemoryLeaseTable<N> {
    const fn new(memory_len: u32, epoch: u32) -> Self {
        Self {
            memory_len,
            epoch,
            slots: [None; N],
        }
    }

    fn has_outstanding_leases(&self) -> bool {
        let mut idx = 0usize;
        while idx < N {
            if self.slots[idx].is_some() {
                return true;
            }
            idx += 1;
        }
        false
    }

    fn grant_read(&mut self, borrow: MemBorrow) -> Result<MemGrant, MemoryLeaseError> {
        if borrow.len() == 0 {
            return Err(MemoryLeaseError::Empty);
        }
        if borrow.len() as usize > WASIP2_STREAM_CHUNK_CAPACITY {
            return Err(MemoryLeaseError::TooLarge);
        }
        if borrow.epoch() != self.epoch {
            return Err(MemoryLeaseError::EpochMismatch);
        }
        let end = borrow
            .ptr()
            .checked_add(borrow.len() as u32)
            .ok_or(MemoryLeaseError::OutOfBounds)?;
        if end > self.memory_len {
            return Err(MemoryLeaseError::OutOfBounds);
        }
        let mut slot_index = None;
        let mut idx = 0usize;
        while idx < N {
            if self.slots[idx].is_none() {
                slot_index = Some(idx);
                break;
            }
            idx += 1;
        }
        let slot_index = slot_index.ok_or(MemoryLeaseError::TableFull)?;
        let lease_id = self.allocate_lease_id()?;
        let grant = MemGrant::new(
            lease_id,
            borrow.ptr(),
            borrow.len(),
            borrow.epoch(),
            MemRights::Read,
        );
        self.slots[slot_index] = Some(grant);
        Ok(grant)
    }

    fn validate_read_chunk(&self, chunk: &Wasip2StreamChunk) -> Result<(), MemoryLeaseError> {
        let grant = self.get(chunk.lease_id())?;
        if grant.rights() != MemRights::Read {
            return Err(MemoryLeaseError::RightsMismatch);
        }
        if chunk.len() > grant.len() as usize {
            return Err(MemoryLeaseError::LengthExceeded);
        }
        Ok(())
    }

    fn release(&mut self, release: MemRelease) -> Result<MemGrant, MemoryLeaseError> {
        let lease_id = release.lease_id();
        if lease_id == MEM_LEASE_NONE {
            return Err(MemoryLeaseError::InvalidLeaseId);
        }
        let mut idx = 0usize;
        while idx < N {
            if let Some(grant) = self.slots[idx] {
                if grant.lease_id() == lease_id {
                    self.slots[idx] = None;
                    return Ok(grant);
                }
            }
            idx += 1;
        }
        Err(MemoryLeaseError::UnknownLease)
    }

    fn allocate_lease_id(&self) -> Result<u8, MemoryLeaseError> {
        let mut candidate = 1u8;
        loop {
            if candidate != MEM_LEASE_NONE && self.get(candidate).is_err() {
                return Ok(candidate);
            }
            if candidate == u8::MAX {
                break;
            }
            candidate += 1;
        }
        Err(MemoryLeaseError::TableFull)
    }

    fn get(&self, lease_id: u8) -> Result<MemGrant, MemoryLeaseError> {
        if lease_id == MEM_LEASE_NONE {
            return Err(MemoryLeaseError::InvalidLeaseId);
        }
        let mut idx = 0usize;
        while idx < N {
            if let Some(grant) = self.slots[idx] {
                if grant.lease_id() == lease_id {
                    return Ok(grant);
                }
            }
            idx += 1;
        }
        Err(MemoryLeaseError::UnknownLease)
    }
}

macro_rules! seq_chain {
    ($head:expr, $($tail:expr),+ $(,)?) => {
        g::seq($head, seq_chain!($($tail),+))
    };
    ($last:expr $(,)?) => {
        $last
    };
}

macro_rules! memory_control_tx {
    () => {
        seq_chain!(
            g::send::<Role<1>, Role<0>, Msg<MEM_BORROW_READ_LOGICAL, MemBorrow>, 1>(),
            g::send::<Role<0>, Role<1>, MemReadGrantControl, 1>(),
            g::send::<Role<1>, Role<0>, Msg<WASIP2_STDOUT_LOGICAL, EngineReq>, 1>(),
            g::send::<Role<0>, Role<1>, Msg<WASIP2_STDOUT_RET_LOGICAL, EngineRet>, 1>(),
            g::send::<Role<1>, Role<0>, Msg<MEM_RELEASE_LOGICAL, MemRelease>, 1>(),
        )
    };
}

#[cfg(feature = "memory-control-1")]
macro_rules! memory_control_program {
    () => {
        memory_control_tx!()
    };
}

#[cfg(feature = "memory-control-4")]
macro_rules! memory_control_program {
    () => {
        seq_chain!(
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
        )
    };
}

#[cfg(feature = "memory-control-8")]
macro_rules! memory_control_program {
    () => {
        seq_chain!(
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
            memory_control_tx!(),
        )
    };
}

pub fn controller_program() -> RoleProgram<0> {
    let program = memory_control_program!();
    let projected: RoleProgram<0> = project(&program);
    projected
}

pub fn worker_program() -> RoleProgram<1> {
    let program = memory_control_program!();
    let projected: RoleProgram<1> = project(&program);
    projected
}

pub fn run(
    controller: &mut localside::ControllerEndpoint<'_>,
    worker: &mut localside::WorkerEndpoint<'_>,
) {
    let mut leases: MemoryLeaseTable<1> = MemoryLeaseTable::new(4096, TEST_MEMORY_EPOCH);
    let stdout = must(StdoutChunk::new(TEST_STDOUT_BYTES));
    let mut idx = 0usize;
    while idx < MATRIX_COUNT {
        let borrow = MemBorrow::new(
            TEST_STDOUT_PTR + (idx as u32) * 32,
            stdout.len() as u8,
            TEST_MEMORY_EPOCH,
        );
        must(localside::drive(
            worker
                .flow::<Msg<MEM_BORROW_READ_LOGICAL, MemBorrow>>()
                .expect("worker flow<mem borrow read>")
                .send(&borrow),
        ));

        let received_borrow = must(localside::drive(
            controller.recv::<Msg<MEM_BORROW_READ_LOGICAL, MemBorrow>>(),
        ));
        assert_eq!(received_borrow, borrow);
        let grant = must(leases.grant_read(received_borrow));
        must(localside::drive(
            controller
                .flow::<MemReadGrantControl>()
                .expect("controller flow<mem read grant control>")
                .send(()),
        ));

        let received_grant = must(localside::drive(worker.recv::<MemReadGrantControl>()));
        let (rights, lease_id) = must(received_grant.decode_handle());
        assert_eq!(rights, MemRights::Read.tag());
        assert_eq!(lease_id as u8, grant.lease_id());

        let request = EngineReq::Wasip2Stdout(stdout.with_lease(lease_id as u8));
        must(localside::drive(
            worker
                .flow::<Msg<WASIP2_STDOUT_LOGICAL, EngineReq>>()
                .expect("worker flow<stdout>")
                .send(&request),
        ));

        let received = must(localside::drive(
            controller.recv::<Msg<WASIP2_STDOUT_LOGICAL, EngineReq>>(),
        ));
        assert_eq!(received, request);
        let EngineReq::Wasip2Stdout(received_chunk) = received;
        must(leases.validate_read_chunk(&received_chunk));

        let reply = EngineRet::Wasip2StdoutWritten(received_chunk.len() as u8);
        must(localside::drive(
            controller
                .flow::<Msg<WASIP2_STDOUT_RET_LOGICAL, EngineRet>>()
                .expect("controller flow<stdout ret>")
                .send(&reply),
        ));

        let received_reply = must(localside::drive(
            worker.recv::<Msg<WASIP2_STDOUT_RET_LOGICAL, EngineRet>>(),
        ));
        assert_eq!(received_reply, reply);

        let release = MemRelease::new(lease_id as u8);
        must(localside::drive(
            worker
                .flow::<Msg<MEM_RELEASE_LOGICAL, MemRelease>>()
                .expect("worker flow<mem release>")
                .send(&release),
        ));
        let received_release = must(localside::drive(
            controller.recv::<Msg<MEM_RELEASE_LOGICAL, MemRelease>>(),
        ));
        assert_eq!(received_release, release);
        must(leases.release(received_release));
        assert!(!leases.has_outstanding_leases());
        idx += 1;
    }
}

fn must<T, E>(value: Result<T, E>) -> T {
    match value {
        Ok(value) => value,
        Err(_) => loop {
            core::hint::spin_loop();
        },
    }
}
