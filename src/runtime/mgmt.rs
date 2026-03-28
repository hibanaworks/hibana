//! Runtime management session — typestate-first, macro-free, allocation-free.
//!
//! `Manager<State, const SLOTS>` models the typestate automaton that drives the
//! EPF VM management session. Bytecode load/commit/activate/rollback operations
//! are only available on the corresponding state variants, and the chunk stream
//! is statically checked through `ChunkCursor` using stable Rust. Memory backing
//! is provided by the rendezvous-resident `SlotArena`/`HostSlots` and is passed
//! in by the caller when constructing the manager.

/// Internal management kernel choreography.
mod kernel;

use core::marker::PhantomData;

use crate::{
    control::{
        lease::{
            bundle::LeaseBundleFacet,
            core::{ControlAutomaton, ControlStep, RendezvousLease, SlotSpec},
            graph::{LeaseGraph, LeaseSpec},
            planner::{LeaseFacetNeeds, LeaseSpecFacetNeeds, facets_slots},
        },
        types::RendezvousId,
    },
    epf::{
        PolicyMode,
        host::{HostError, HostSlots, Machine},
        loader::{ImageLoader, LoaderError},
        verifier::{Header, VerifyError},
        vm::Slot,
    },
    observe::{
        core::{TAP_BATCH_MAX_EVENTS, TapBatch, TapEvent, push},
        events::{PolicyCommit, PolicyRollback},
    },
    rendezvous::slots::{SLOT_COUNT, SlotStorage, slot_index},
    transport::wire::{CodecError, WireDecode, WireEncode},
};

const MGMT_FACET_NEEDS: LeaseFacetNeeds = facets_slots();
const LOAD_CHUNK_MAX: usize = 1024;

#[cfg(test)]
pub(crate) fn management_compiled_programs() -> (
    crate::global::compiled::CompiledProgram,
    crate::global::compiled::CompiledProgram,
) {
    kernel::management_compiled_programs()
}

pub(crate) fn enter_controller<'cfg, T, U, C, B, const MAX_RV: usize>(
    cluster: &'cfg crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    binding: B,
) -> Result<
    crate::Endpoint<
        'cfg,
        0,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        B,
    >,
    crate::control::cluster::error::AttachError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    B: crate::binding::BindingSlot,
{
    kernel::enter_controller(cluster, rv_id, sid, binding)
}

pub(crate) fn enter_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(
    cluster: &'cfg crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    binding: B,
) -> Result<
    crate::Endpoint<
        'cfg,
        1,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        B,
    >,
    crate::control::cluster::error::AttachError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    B: crate::binding::BindingSlot,
{
    kernel::enter_cluster(cluster, rv_id, sid, binding)
}

pub(crate) fn enter_stream_controller<'cfg, T, U, C, B, const MAX_RV: usize>(
    cluster: &'cfg crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    binding: B,
) -> Result<
    crate::Endpoint<
        'cfg,
        0,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        B,
    >,
    crate::control::cluster::error::AttachError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    B: crate::binding::BindingSlot,
{
    kernel::enter_stream_controller(cluster, rv_id, sid, binding)
}

pub(crate) fn enter_stream_cluster<'cfg, T, U, C, B, const MAX_RV: usize>(
    cluster: &'cfg crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    binding: B,
) -> Result<
    crate::Endpoint<
        'cfg,
        1,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        crate::control::cap::mint::MintConfig,
        B,
    >,
    crate::control::cluster::error::AttachError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    B: crate::binding::BindingSlot,
{
    kernel::enter_stream_cluster(cluster, rv_id, sid, binding)
}

pub(crate) async fn drive_controller<'lease, 'request, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: crate::Endpoint<
        'lease,
        0,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    request: Request<'request>,
) -> Result<
    (
        crate::Endpoint<'lease, 0, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV, Mint, B>,
        Reply,
    ),
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: crate::control::cap::mint::MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, reply) = kernel::drive_controller(endpoint.into_cursor(), request).await?;
    Ok((crate::Endpoint::from_cursor(endpoint), reply))
}

pub(crate) async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(
    cluster: &'lease crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    endpoint: crate::Endpoint<
        'lease,
        1,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    crate::Endpoint<'lease, 1, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV, Mint, B>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    Mint: crate::control::cap::mint::MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let endpoint = kernel::drive_cluster(cluster, rv_id, sid, endpoint.into_cursor()).await?;
    Ok(crate::Endpoint::from_cursor(endpoint))
}

fn apply_seed<'cfg, T, U, C, const MAX_RV: usize>(
    cluster: &crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    seed: MgmtSeed<AwaitBegin>,
) -> Result<Reply, MgmtError>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    cluster.drive_mgmt(rv_id, sid, seed)
}

pub(crate) fn apply_request_action<'cfg, T, U, C, const MAX_RV: usize>(
    cluster: &crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    backup: Manager<AwaitBegin, SLOT_COUNT>,
    manager: Manager<AwaitBegin, SLOT_COUNT>,
    request: RequestAction,
) -> Result<Reply, MgmtError>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
{
    let seed = MgmtSeed {
        request,
        manager,
        links: MgmtLinks::new(),
    };
    match apply_seed(cluster, rv_id, sid, seed) {
        Ok(reply) => Ok(reply),
        Err(err) => {
            cluster.store_mgmt_manager(rv_id, backup);
            Err(err)
        }
    }
}

pub(crate) async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
    endpoint: crate::Endpoint<
        'lease,
        1,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    should_continue: F,
) -> Result<
    crate::Endpoint<'lease, 1, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV, Mint, B>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: crate::control::cap::mint::MintConfigMarker,
    Mint::Policy: crate::control::cap::mint::AllowsCanonical,
    F: FnMut() -> bool,
    B: crate::binding::BindingSlot,
{
    let endpoint = kernel::drive_stream_cluster(endpoint.into_cursor(), should_continue).await?;
    Ok(crate::Endpoint::from_cursor(endpoint))
}

pub(crate) async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
    endpoint: crate::Endpoint<
        'lease,
        0,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    subscribe: SubscribeReq,
    on_event: F,
) -> Result<
    crate::Endpoint<'lease, 0, T, U, C, crate::control::cap::mint::EpochTbl, MAX_RV, Mint, B>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: crate::control::cap::mint::MintConfigMarker,
    F: FnMut(TapEvent) -> bool,
    B: crate::binding::BindingSlot,
{
    let endpoint =
        kernel::drive_stream_controller(endpoint.into_cursor(), subscribe, on_event).await?;
    Ok(crate::Endpoint::from_cursor(endpoint))
}

/// Errors that can occur during the management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MgmtError {
    InvalidSlot(u8),
    InvalidTransition,
    ChunkOutOfOrder { expected: u32, got: u32 },
    ChunkTooLarge { remaining: u32, provided: u32 },
    LoaderNotFinalised,
    NoStagedImage,
    NoActiveImage,
    NoPreviousImage,
    CapabilityMismatch,
    ObserveUnavailable,
    HostInstallFailed,
    HostUninstallFailed,
    StreamEnded,
}

impl From<LoaderError> for MgmtError {
    fn from(err: LoaderError) -> Self {
        match err {
            LoaderError::AlreadyLoading => MgmtError::InvalidTransition,
            LoaderError::NotLoading => MgmtError::InvalidTransition,
            LoaderError::CodeTooLarge { declared } => MgmtError::ChunkTooLarge {
                remaining: 0,
                provided: declared as u32,
            },
            LoaderError::UnexpectedOffset { expected, got } => {
                MgmtError::ChunkOutOfOrder { expected, got }
            }
            LoaderError::ChunkTooLarge {
                remaining,
                provided,
            } => MgmtError::ChunkTooLarge {
                remaining,
                provided,
            },
            LoaderError::HashMismatch { .. } => MgmtError::LoaderNotFinalised,
            LoaderError::Verify(err) => MgmtError::from(err),
        }
    }
}

impl From<VerifyError> for MgmtError {
    fn from(_: VerifyError) -> Self {
        MgmtError::LoaderNotFinalised
    }
}

impl From<HostError> for MgmtError {
    fn from(err: HostError) -> Self {
        match err {
            HostError::SlotOccupied => MgmtError::InvalidTransition,
            HostError::SlotEmpty => MgmtError::HostUninstallFailed,
            HostError::InvalidFuel => MgmtError::HostInstallFailed,
            HostError::ScratchTooSmall { .. } => MgmtError::HostInstallFailed,
            HostError::ScratchTooLarge { .. } => MgmtError::HostInstallFailed,
        }
    }
}

impl WireEncode for MgmtError {
    fn encoded_len(&self) -> Option<usize> {
        Some(match self {
            MgmtError::InvalidSlot(_) => 2,
            MgmtError::ChunkOutOfOrder { .. } => 9,
            MgmtError::ChunkTooLarge { .. } => 9,
            _ => 1,
        })
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let need = self.encoded_len().unwrap_or(1);
        if out.len() < need {
            return Err(CodecError::Truncated);
        }
        match self {
            MgmtError::InvalidSlot(slot) => {
                out[0] = 0;
                out[1] = *slot;
            }
            MgmtError::InvalidTransition => out[0] = 1,
            MgmtError::ChunkOutOfOrder { expected, got } => {
                out[0] = 2;
                out[1..5].copy_from_slice(&expected.to_be_bytes());
                out[5..9].copy_from_slice(&got.to_be_bytes());
            }
            MgmtError::ChunkTooLarge {
                remaining,
                provided,
            } => {
                out[0] = 3;
                out[1..5].copy_from_slice(&remaining.to_be_bytes());
                out[5..9].copy_from_slice(&provided.to_be_bytes());
            }
            MgmtError::LoaderNotFinalised => out[0] = 4,
            MgmtError::NoStagedImage => out[0] = 5,
            MgmtError::NoActiveImage => out[0] = 6,
            MgmtError::NoPreviousImage => out[0] = 7,
            MgmtError::CapabilityMismatch => out[0] = 8,
            MgmtError::ObserveUnavailable => out[0] = 9,
            MgmtError::HostInstallFailed => out[0] = 10,
            MgmtError::HostUninstallFailed => out[0] = 11,
            MgmtError::StreamEnded => out[0] = 12,
        }
        Ok(need)
    }
}

impl<'a> WireDecode<'a> for MgmtError {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        match input[0] {
            0 => {
                if input.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::InvalidSlot(input[1]))
            }
            1 => Ok(MgmtError::InvalidTransition),
            2 => {
                if input.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::ChunkOutOfOrder {
                    expected: u32::from_be_bytes([input[1], input[2], input[3], input[4]]),
                    got: u32::from_be_bytes([input[5], input[6], input[7], input[8]]),
                })
            }
            3 => {
                if input.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                Ok(MgmtError::ChunkTooLarge {
                    remaining: u32::from_be_bytes([input[1], input[2], input[3], input[4]]),
                    provided: u32::from_be_bytes([input[5], input[6], input[7], input[8]]),
                })
            }
            4 => Ok(MgmtError::LoaderNotFinalised),
            5 => Ok(MgmtError::NoStagedImage),
            6 => Ok(MgmtError::NoActiveImage),
            7 => Ok(MgmtError::NoPreviousImage),
            8 => Ok(MgmtError::CapabilityMismatch),
            9 => Ok(MgmtError::ObserveUnavailable),
            10 => Ok(MgmtError::HostInstallFailed),
            11 => Ok(MgmtError::HostUninstallFailed),
            12 => Ok(MgmtError::StreamEnded),
            _ => Err(CodecError::Invalid("unknown management error tag")),
        }
    }
}

/// Typical management protocol replies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Loaded(LoadReport),
    ActivationScheduled(TransitionReport),
    Reverted(TransitionReport),
    Stats {
        stats: StatsResp,
        staged_version: Option<u32>,
    },
}

/// One-shot report returned when an image is staged but not activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadReport {
    pub staged_version: u32,
}

/// Payload carried by the stats reply route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsReply {
    pub stats: StatsResp,
    pub staged_version: Option<u32>,
}

/// Request payload for code upload branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadRequest<'a> {
    pub slot: crate::substrate::policy::epf::Slot,
    pub code: &'a [u8],
    pub fuel_max: u16,
    pub mem_len: u16,
}

/// Request payload for slot-scoped command branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRequest {
    pub slot: crate::substrate::policy::epf::Slot,
}

/// Management requests carried by the request/reply management session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    Load(LoadRequest<'a>),
    LoadAndActivate(LoadRequest<'a>),
    Activate(SlotRequest),
    Revert(SlotRequest),
    Stats(SlotRequest),
}

/// Slot-level metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatsResp {
    pub traps: u32,
    pub aborts: u32,
    pub fuel_used: u32,
    pub active_version: u32,
}

/// Policy statistics collected during transitions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionReport {
    pub version: u32,
    pub policy_stats: PolicyStats,
}

/// Policy event statistics harvested from tap events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PolicyStats {
    pub aborts: u32,
    pub traps: u32,
    pub annotations: u32,
    pub effects: u32,
    pub effects_ok: u32,
    pub commits: u32,
    pub rollbacks: u32,
    pub last_commit: Option<u32>,
    pub last_rollback: Option<u32>,
}

/// Per-slot digest pointers for O(1) activation/revert bookkeeping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PolicyDigestState {
    pub active_digest: Option<u32>,
    pub standby_digest: Option<u32>,
    pub last_good_digest: Option<u32>,
}

#[cfg(test)]
/// Promotion gate thresholds for Shadow → Enforce rollout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PromotionGateThresholds {
    pub min_samples: u32,
    pub max_divergence_ppm: u32,
    pub max_reject_delta_ppm: u32,
    pub max_p99_eval_us: u32,
    pub max_latency_increase_ppm: u32,
    pub max_fail_closed_ppm: u32,
    pub required_consecutive_windows: u8,
}

#[cfg(test)]
impl Default for PromotionGateThresholds {
    fn default() -> Self {
        Self {
            min_samples: 10_000,
            max_divergence_ppm: 1_000, // 0.10%
            max_reject_delta_ppm: 500, // 0.05%
            max_p99_eval_us: 250,
            max_latency_increase_ppm: 200_000, // +20%
            max_fail_closed_ppm: 100,          // 0.01%
            required_consecutive_windows: 3,
        }
    }
}

#[cfg(test)]
/// Single promotion-gate measurement window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PromotionGateWindow {
    pub sample_count: u32,
    pub divergence_ppm: u32,
    pub reject_delta_ppm: u32,
    pub p99_eval_us: u32,
    pub latency_increase_ppm: u32,
    pub fail_closed_ppm: u32,
}

#[cfg(test)]
/// Running promotion-gate status for a slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PromotionGateState {
    pub consecutive_windows: u8,
}

#[cfg(test)]
impl PromotionGateState {
    #[inline]
    pub(crate) fn observe(
        &mut self,
        window: PromotionGateWindow,
        thresholds: PromotionGateThresholds,
    ) -> bool {
        let pass = window.sample_count >= thresholds.min_samples
            && window.divergence_ppm <= thresholds.max_divergence_ppm
            && window.reject_delta_ppm <= thresholds.max_reject_delta_ppm
            && window.p99_eval_us <= thresholds.max_p99_eval_us
            && window.latency_increase_ppm <= thresholds.max_latency_increase_ppm
            && window.fail_closed_ppm <= thresholds.max_fail_closed_ppm;
        if pass {
            self.consecutive_windows = self.consecutive_windows.saturating_add(1);
        } else {
            self.consecutive_windows = 0;
        }
        self.consecutive_windows >= thresholds.required_consecutive_windows
    }
}

/// Payload carried by the `LoadBegin` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadBegin {
    pub slot: crate::substrate::policy::epf::Slot,
    pub code_len: u32,
    pub fuel_max: u16,
    pub mem_len: u16,
    pub hash: u32,
}

/// Payload for `LoadChunk`; the chunk body lives in a fixed-size buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadChunk {
    pub offset: u32,
    pub len: u16,
    pub bytes: [u8; LOAD_CHUNK_MAX],
}

impl LoadChunk {
    pub fn new(offset: u32, chunk: &[u8]) -> Self {
        assert!(
            chunk.len() <= LOAD_CHUNK_MAX,
            "chunk length exceeds management chunk capacity"
        );
        let mut bytes = [0u8; LOAD_CHUNK_MAX];
        bytes[..chunk.len()].copy_from_slice(chunk);
        Self {
            offset,
            len: chunk.len() as u16,
            bytes,
        }
    }

    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// Subscribe request for streaming observe (single-event streaming).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubscribeReq {
    /// Reserved for future use (filter flags, etc.)
    pub flags: u16,
}

impl WireEncode for SubscribeReq {
    fn encoded_len(&self) -> Option<usize> {
        Some(2)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.flags.to_be_bytes());
        Ok(2)
    }
}

impl<'a> WireDecode<'a> for SubscribeReq {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 2 {
            return Err(CodecError::Truncated);
        }
        let flags = u16::from_be_bytes([input[0], input[1]]);
        Ok(SubscribeReq { flags })
    }
}

// TapEvent wire encoding: 20 bytes
// ts(4) + id(2) + causal_key(2) + arg0(4) + arg1(4) + arg2(4)
const TAP_EVENT_WIRE_LEN: usize = 20;

impl WireEncode for TapEvent {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.ts.to_be_bytes());
        out[4..6].copy_from_slice(&self.id.to_be_bytes());
        out[6..8].copy_from_slice(&self.causal_key.to_be_bytes());
        out[8..12].copy_from_slice(&self.arg0.to_be_bytes());
        out[12..16].copy_from_slice(&self.arg1.to_be_bytes());
        out[16..20].copy_from_slice(&self.arg2.to_be_bytes());
        Ok(TAP_EVENT_WIRE_LEN)
    }
}

impl<'a> WireDecode<'a> for TapEvent {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_EVENT_WIRE_LEN {
            return Err(CodecError::Truncated);
        }
        Ok(TapEvent {
            ts: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            id: u16::from_be_bytes([input[4], input[5]]),
            causal_key: u16::from_be_bytes([input[6], input[7]]),
            arg0: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            arg1: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
            arg2: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
        })
    }
}

// TapBatch wire encoding: 5 + (count × 20) bytes
// count(1) + lost_events(4) + events(count × 20)
const TAP_BATCH_HEADER_LEN: usize = 5;

impl WireEncode for TapBatch {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let total_len = TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN;
        if out.len() < total_len {
            return Err(CodecError::Truncated);
        }

        out[0] = self.len() as u8;
        out[1..5].copy_from_slice(&self.lost_events().to_be_bytes());

        let mut offset = TAP_BATCH_HEADER_LEN;
        for event in self.iter() {
            event.encode_into(&mut out[offset..])?;
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(total_len)
    }
}

impl<'a> WireDecode<'a> for TapBatch {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_BATCH_HEADER_LEN {
            return Err(CodecError::Truncated);
        }

        let count = input[0] as usize;
        if count > TAP_BATCH_MAX_EVENTS {
            return Err(CodecError::Invalid("batch count exceeds maximum"));
        }

        let lost_events = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let expected_len = TAP_BATCH_HEADER_LEN + count * TAP_EVENT_WIRE_LEN;
        if input.len() < expected_len {
            return Err(CodecError::Truncated);
        }

        let mut batch = TapBatch::empty();
        batch.set_lost_events(lost_events);

        let mut offset = TAP_BATCH_HEADER_LEN;
        for _ in 0..count {
            let event = TapEvent::decode_from(&input[offset..])?;
            batch.push(event);
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(batch)
    }
}

impl WireEncode for StatsResp {
    fn encoded_len(&self) -> Option<usize> {
        Some(16)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 16 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.traps.to_be_bytes());
        out[4..8].copy_from_slice(&self.aborts.to_be_bytes());
        out[8..12].copy_from_slice(&self.fuel_used.to_be_bytes());
        out[12..16].copy_from_slice(&self.active_version.to_be_bytes());
        Ok(16)
    }
}

impl<'a> WireDecode<'a> for StatsResp {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 16 {
            return Err(CodecError::Truncated);
        }
        Ok(StatsResp {
            traps: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            aborts: u32::from_be_bytes([input[4], input[5], input[6], input[7]]),
            fuel_used: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            active_version: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
        })
    }
}

impl WireEncode for PolicyStats {
    fn encoded_len(&self) -> Option<usize> {
        Some(38)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 36 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.aborts.to_be_bytes());
        out[4..8].copy_from_slice(&self.traps.to_be_bytes());
        out[8..12].copy_from_slice(&self.annotations.to_be_bytes());
        out[12..16].copy_from_slice(&self.effects.to_be_bytes());
        out[16..20].copy_from_slice(&self.effects_ok.to_be_bytes());
        out[20..24].copy_from_slice(&self.commits.to_be_bytes());
        out[24..28].copy_from_slice(&self.rollbacks.to_be_bytes());
        out[28..32].copy_from_slice(&self.last_commit.unwrap_or(0).to_be_bytes());
        out[32] = u8::from(self.last_commit.is_some());
        out[33..37].copy_from_slice(&self.last_rollback.unwrap_or(0).to_be_bytes());
        out[37] = u8::from(self.last_rollback.is_some());
        Ok(38)
    }
}

impl<'a> WireDecode<'a> for PolicyStats {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 38 {
            return Err(CodecError::Truncated);
        }
        let last_commit = u32::from_be_bytes([input[28], input[29], input[30], input[31]]);
        let last_rollback = u32::from_be_bytes([input[33], input[34], input[35], input[36]]);
        Ok(PolicyStats {
            aborts: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            traps: u32::from_be_bytes([input[4], input[5], input[6], input[7]]),
            annotations: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
            effects: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
            effects_ok: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
            commits: u32::from_be_bytes([input[20], input[21], input[22], input[23]]),
            rollbacks: u32::from_be_bytes([input[24], input[25], input[26], input[27]]),
            last_commit: if input[32] == 0 {
                None
            } else {
                Some(last_commit)
            },
            last_rollback: if input[37] == 0 {
                None
            } else {
                Some(last_rollback)
            },
        })
    }
}

impl WireEncode for TransitionReport {
    fn encoded_len(&self) -> Option<usize> {
        Some(42)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 42 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.version.to_be_bytes());
        self.policy_stats.encode_into(&mut out[4..])?;
        Ok(42)
    }
}

impl<'a> WireDecode<'a> for TransitionReport {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 42 {
            return Err(CodecError::Truncated);
        }
        Ok(TransitionReport {
            version: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
            policy_stats: PolicyStats::decode_from(&input[4..42])?,
        })
    }
}

impl WireEncode for LoadReport {
    fn encoded_len(&self) -> Option<usize> {
        Some(4)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.staged_version.to_be_bytes());
        Ok(4)
    }
}

impl<'a> WireDecode<'a> for LoadReport {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 4 {
            return Err(CodecError::Truncated);
        }
        Ok(LoadReport {
            staged_version: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
        })
    }
}

impl WireEncode for StatsReply {
    fn encoded_len(&self) -> Option<usize> {
        Some(21)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 21 {
            return Err(CodecError::Truncated);
        }
        self.stats.encode_into(out)?;
        out[16] = u8::from(self.staged_version.is_some());
        out[17..21].copy_from_slice(&self.staged_version.unwrap_or(0).to_be_bytes());
        Ok(21)
    }
}

impl<'a> WireDecode<'a> for StatsReply {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 21 {
            return Err(CodecError::Truncated);
        }
        let stats = StatsResp::decode_from(&input[..16])?;
        let staged_version = if input[16] == 0 {
            None
        } else {
            Some(u32::from_be_bytes([
                input[17], input[18], input[19], input[20],
            ]))
        };
        Ok(StatsReply {
            stats,
            staged_version,
        })
    }
}

impl WireEncode for LoadBegin {
    fn encoded_len(&self) -> Option<usize> {
        Some(13)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 13 {
            return Err(CodecError::Truncated);
        }
        out[0] = slot_id(self.slot) as u8;
        out[1..5].copy_from_slice(&self.code_len.to_be_bytes());
        out[5..7].copy_from_slice(&self.fuel_max.to_be_bytes());
        out[7..9].copy_from_slice(&self.mem_len.to_be_bytes());
        out[9..13].copy_from_slice(&self.hash.to_be_bytes());
        Ok(13)
    }
}

impl<'a> WireDecode<'a> for LoadBegin {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 13 {
            return Err(CodecError::Truncated);
        }
        let slot = decode_slot(input[0])?;
        let code_len = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let fuel_max = u16::from_be_bytes([input[5], input[6]]);
        let mem_len = u16::from_be_bytes([input[7], input[8]]);
        let hash = u32::from_be_bytes([input[9], input[10], input[11], input[12]]);
        Ok(LoadBegin {
            slot,
            code_len,
            fuel_max,
            mem_len,
            hash,
        })
    }
}

impl WireEncode for LoadChunk {
    fn encoded_len(&self) -> Option<usize> {
        Some(6 + self.len as usize)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len as usize;
        if len > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        let total = 6 + len;
        if out.len() < total {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.offset.to_be_bytes());
        out[4..6].copy_from_slice(&self.len.to_be_bytes());
        out[6..total].copy_from_slice(&self.bytes[..len]);
        Ok(total)
    }
}

impl<'a> WireDecode<'a> for LoadChunk {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < 6 {
            return Err(CodecError::Truncated);
        }
        let offset = u32::from_be_bytes([input[0], input[1], input[2], input[3]]);
        let len = u16::from_be_bytes([input[4], input[5]]);
        let len_usize = len as usize;
        if len_usize > LOAD_CHUNK_MAX {
            return Err(CodecError::Invalid("chunk length exceeds LOAD_CHUNK_MAX"));
        }
        if input.len() < 6 + len_usize {
            return Err(CodecError::Truncated);
        }
        let mut bytes = [0u8; LOAD_CHUNK_MAX];
        bytes[..len_usize].copy_from_slice(&input[6..6 + len_usize]);
        Ok(LoadChunk { offset, len, bytes })
    }
}

impl WireEncode for SlotRequest {
    fn encoded_len(&self) -> Option<usize> {
        Some(1)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = slot_id(self.slot) as u8;
        Ok(1)
    }
}

impl<'a> WireDecode<'a> for SlotRequest {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        Ok(SlotRequest {
            slot: decode_slot(input[0])?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cold;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AwaitBegin;

pub(crate) trait ManagerState {}
impl ManagerState for Cold {}
impl ManagerState for AwaitBegin {}

/// Per-slot state.
#[derive(Clone)]
struct SlotState {
    loader: ImageLoader,
    inventory: SlotInventory,
    pending: PendingVersion,
    version_counter: u32,
    active_epoch: u32,
    pending_epoch: Option<u32>,
    last_policy_stats: PolicyStats,
    digest_state: PolicyDigestState,
    policy_mode: PolicyMode,
    #[cfg(test)]
    promotion_gate: PromotionGateState,
}

impl SlotState {
    fn new() -> Self {
        Self {
            loader: ImageLoader::new(),
            inventory: SlotInventory::new(),
            pending: PendingVersion::default(),
            version_counter: 0,
            active_epoch: 0,
            pending_epoch: None,
            last_policy_stats: PolicyStats::default(),
            digest_state: PolicyDigestState::default(),
            policy_mode: PolicyMode::Shadow,
            #[cfg(test)]
            promotion_gate: PromotionGateState::default(),
        }
    }
}

/// Typestate-driven management session that stages, activates, and rolls back
/// EPF policy images.
#[derive(Clone)]
pub(crate) struct Manager<State, const SLOTS: usize>
where
    State: ManagerState,
{
    slot_states: [SlotState; SLOTS],
    _state: PhantomData<State>,
    timestamp: u32,
}

impl<const SLOTS: usize> Default for Manager<Cold, SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Manager<Cold, SLOTS> {
    /// Construct a management session manager in the initial `Cold` state.
    ///
    /// The manager records the current tap cursor so subsequent policy
    /// metrics can be correlated with the tap stream.
    pub(crate) fn new() -> Self {
        Self {
            slot_states: core::array::from_fn(|_| SlotState::new()),
            _state: PhantomData,
            timestamp: 0,
        }
    }

    /// Transition into `AwaitBegin`, enabling the load handshake.
    pub(crate) fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {
        let ts = self.timestamp;
        self.transition(AwaitBegin, ts)
    }
}

impl<State, const SLOTS: usize> Manager<State, SLOTS>
where
    State: ManagerState,
{
    fn transition<Next: ManagerState>(self, _state: Next, timestamp: u32) -> Manager<Next, SLOTS> {
        let Manager { slot_states, .. } = self;
        Manager {
            slot_states,
            _state: PhantomData,
            timestamp,
        }
    }

    fn slot_state(&mut self, slot: Slot) -> &mut SlotState {
        &mut self.slot_states[slot_index(slot)]
    }

    fn next_ts(&mut self) -> u32 {
        let current = self.timestamp;
        self.timestamp = self.timestamp.saturating_add(1);
        current
    }

    pub(crate) fn load_begin(&mut self, slot: Slot, header: Header) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        state.loader.begin(header)?;
        let version = state.version_counter.saturating_add(1);
        state.pending.begin(version);
        Ok(version)
    }

    pub(crate) fn load_chunk(
        &mut self,
        slot: Slot,
        offset: u32,
        chunk: &[u8],
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.loader.write(offset, chunk)?;
        Ok(())
    }

    pub(crate) fn load_commit(
        &mut self,
        slot: Slot,
        storage: &mut SlotStorage,
    ) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        let version = state.pending.take()?;
        let verified = state.loader.commit_for_slot(slot)?;
        let code = verified.code;
        storage.staging_mut()[..code.len()].copy_from_slice(code);

        let mut header = verified.header;
        header.code_len = code.len() as u16;
        let meta = ImageMeta::new(version, header);
        state.digest_state.standby_digest = Some(meta.header.hash);
        state.inventory.stage(meta);
        state.version_counter = version;
        Ok(version)
    }

    pub(crate) fn schedule_activate(&mut self, slot: Slot) -> Result<TransitionReport, MgmtError> {
        let state = self.slot_state(slot);
        let staged = state.inventory.staged().ok_or(MgmtError::NoStagedImage)?;
        // Activation is boundary-driven on purpose: applying at arbitrary points
        // would violate the offer->route->decode lease consistency contract.
        state.pending_epoch = Some(staged.version);
        Ok(TransitionReport {
            version: staged.version,
            policy_stats: state.last_policy_stats,
        })
    }

    pub(crate) fn on_decision_boundary<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<Option<TransitionReport>, MgmtError> {
        let pending_epoch = {
            let state = self.slot_state(slot);
            state.pending_epoch
        };
        let Some(pending_epoch) = pending_epoch else {
            return Ok(None);
        };
        let staged_version = {
            let state = self.slot_state(slot);
            state.inventory.staged().map(|meta| meta.version)
        };
        if staged_version != Some(pending_epoch) {
            let state = self.slot_state(slot);
            state.pending_epoch = None;
            return Ok(None);
        }
        let report = self.activate_committed(slot, storage, host_slots)?;
        Ok(Some(report))
    }

    fn activate_committed<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<TransitionReport, MgmtError> {
        let ts = self.next_ts();
        let state = self.slot_state(slot);
        let previous_active = state.inventory.current_active();
        let staged = state.inventory.take_stage()?;

        if let Some(active_meta) = previous_active {
            storage.copy_active_to_backup(active_meta.code_len());
            if let Err(err) = host_slots.uninstall(slot)
                && !matches!(err, HostError::SlotEmpty)
            {
                return Err(err.into());
            }
        }

        storage.copy_staging_to_active(staged.meta().code_len());

        let active_meta = staged.meta();
        state.inventory.install_active(active_meta, previous_active);
        state.active_epoch = active_meta.version;
        state.pending_epoch = None;
        if let Some(previous_active_meta) = previous_active {
            state.digest_state.last_good_digest = Some(previous_active_meta.header.hash);
        }
        state.digest_state.active_digest = Some(active_meta.header.hash);
        state.digest_state.standby_digest = None;

        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..active_meta.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            active_meta.header.mem_len as usize,
            active_meta.header.fuel_max,
        )?;
        if let Err(err) = host_slots.install(slot, machine) {
            return Err(err.into());
        }
        host_slots.set_policy_mode(slot, state.policy_mode);

        push(PolicyCommit::with_digest(
            ts,
            slot_id(slot),
            active_meta.version,
            active_meta.header.hash,
        ));
        let policy_stats = PolicyStats {
            commits: 1,
            last_commit: Some(active_meta.version),
            ..PolicyStats::default()
        };
        state.last_policy_stats = policy_stats;
        Ok(TransitionReport {
            version: active_meta.version,
            policy_stats,
        })
    }

    pub(crate) fn revert<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<TransitionReport, MgmtError> {
        let ts = self.next_ts();
        let state = self.slot_state(slot);
        let active_meta = state
            .inventory
            .current_active()
            .ok_or(MgmtError::NoActiveImage)?;

        state.digest_state.standby_digest = Some(active_meta.header.hash);
        storage.copy_active_to_staging(active_meta.code_len());
        state.inventory.stage(active_meta);

        if let Err(err) = host_slots.uninstall(slot)
            && !matches!(err, HostError::SlotEmpty)
        {
            return Err(err.into());
        }

        let new_active = {
            let entry = state.inventory.active_mut()?;
            entry.revert()?
        };
        state.active_epoch = new_active.version;
        state.pending_epoch = None;

        storage.copy_backup_to_active(new_active.code_len());
        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..new_active.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            new_active.header.mem_len as usize,
            new_active.header.fuel_max,
        )?;
        host_slots.install(slot, machine)?;
        host_slots.set_policy_mode(slot, state.policy_mode);
        state.digest_state.active_digest = Some(new_active.header.hash);
        state.digest_state.last_good_digest = Some(new_active.header.hash);

        push(PolicyRollback::with_digest(
            ts,
            slot_id(slot),
            new_active.version,
            new_active.header.hash,
        ));
        let policy_stats = PolicyStats {
            rollbacks: 1,
            last_rollback: Some(new_active.version),
            ..PolicyStats::default()
        };
        state.last_policy_stats = policy_stats;
        Ok(TransitionReport {
            version: new_active.version,
            policy_stats,
        })
    }

    pub(crate) fn stats(&self, slot: Slot) -> Result<StatsResp, MgmtError> {
        let state = &self.slot_states[slot_index(slot)];
        Ok(StatsResp {
            traps: 0,
            aborts: 0,
            fuel_used: 0,
            active_version: state
                .inventory
                .current_active()
                .map(|meta| meta.version)
                .unwrap_or(0),
        })
    }

    pub(crate) fn staged_version(&self, slot: Slot) -> Option<u32> {
        self.slot_states[slot_index(slot)]
            .inventory
            .staged()
            .map(|meta| meta.version)
    }

    #[cfg(test)]
    pub(crate) fn policy_mode(&self, slot: Slot) -> Result<PolicyMode, MgmtError> {
        Ok(self.slot_states[slot_index(slot)].policy_mode)
    }

    #[cfg(test)]
    pub(crate) fn set_policy_mode<'arena>(
        &mut self,
        slot: Slot,
        mode: PolicyMode,
        host_slots: &HostSlots<'arena>,
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.policy_mode = mode;
        host_slots.set_policy_mode(slot, mode);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_policy_mode_staged(
        &mut self,
        slot: Slot,
        mode: PolicyMode,
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.policy_mode = mode;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn observe_promotion_window(
        &mut self,
        slot: Slot,
        window: PromotionGateWindow,
        thresholds: PromotionGateThresholds,
    ) -> Result<bool, MgmtError> {
        let state = self.slot_state(slot);
        Ok(state.promotion_gate.observe(window, thresholds))
    }
}

pub(crate) const ALL_SLOTS: [Slot; SLOT_COUNT] = [
    Slot::Forward,
    Slot::EndpointRx,
    Slot::EndpointTx,
    Slot::Rendezvous,
    Slot::Route,
];

/// Maximum number of rendezvous links tracked by a management seed.
const MGMT_MAX_LINKS: usize = 8;

/// Fixed-capacity set of rendezvous identifiers referenced by the management seed.
#[derive(Clone, Copy)]
pub(crate) struct MgmtLinks {
    ids: [RendezvousId; MGMT_MAX_LINKS],
    len: usize,
}

impl Default for MgmtLinks {
    fn default() -> Self {
        Self::new()
    }
}

impl MgmtLinks {
    /// Construct an empty link set.
    pub(crate) const fn new() -> Self {
        Self {
            ids: [RendezvousId::new(0); MGMT_MAX_LINKS],
            len: 0,
        }
    }

    /// Returns true when the set already contains `id`.
    #[inline]
    pub(crate) fn contains(&self, id: RendezvousId) -> bool {
        (0..self.len).any(|idx| self.ids[idx] == id)
    }

    /// Append a rendezvous identifier if capacity allows and it is not present yet.
    #[inline]
    pub(crate) fn push(&mut self, id: RendezvousId) {
        if self.len >= MGMT_MAX_LINKS || self.contains(id) {
            return;
        }
        self.ids[self.len] = id;
        self.len += 1;
    }

    /// Iterate over the tracked rendezvous identifiers.
    #[inline]
    pub(crate) fn iter(&self) -> impl Iterator<Item = RendezvousId> + '_ {
        self.ids[..self.len].iter().copied()
    }
}

// ======== LeaseGraph integration ============================================

/// Lease specification for management session rendezvous.
pub(crate) struct MgmtLeaseSpec<T, U, C, E>(PhantomData<(T, U, C, E)>);

impl<T, U, C, E> LeaseSpec for MgmtLeaseSpec<T, U, C, E>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type NodeId = RendezvousId;
    type Facet = LeaseBundleFacet<T, U, C, E>;
    const MAX_NODES: usize = MGMT_MAX_LINKS + 1;
    const MAX_CHILDREN: usize = MGMT_MAX_LINKS;
}

impl<T, U, C, E> LeaseSpecFacetNeeds for MgmtLeaseSpec<T, U, C, E>
where
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    #[inline(always)]
    fn facet_needs() -> LeaseFacetNeeds {
        MGMT_FACET_NEEDS
    }
}

/// Seed passed to the management automaton.
pub(crate) struct MgmtSeed<State>
where
    State: ManagerState,
{
    pub(crate) request: RequestAction,
    pub(crate) manager: Manager<State, SLOT_COUNT>,
    pub(crate) links: MgmtLinks,
}

impl<State> MgmtSeed<State>
where
    State: ManagerState,
{
    #[inline]
    pub(crate) fn links_mut(&mut self) -> &mut MgmtLinks {
        &mut self.links
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadMode {
    Stage,
    Activate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestAction {
    Load { slot: Slot, mode: LoadMode },
    Activate { slot: Slot },
    Revert { slot: Slot },
    Stats { slot: Slot },
}

/// Control automaton that applies management operations through LeaseGraph.
pub(crate) struct MgmtAutomaton<State>
where
    State: ManagerState,
{
    _marker: PhantomData<State>,
}

impl<State> Default for MgmtAutomaton<State>
where
    State: ManagerState,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<State> MgmtAutomaton<State>
where
    State: ManagerState,
{
    pub(crate) const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<State, T, U, C, E> ControlAutomaton<T, U, C, E> for MgmtAutomaton<State>
where
    State: ManagerState,
    T: crate::transport::Transport,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    E: crate::control::cap::mint::EpochTable,
{
    type Spec = SlotSpec;
    type Seed = MgmtSeed<State>;
    type Output = (Manager<State, SLOT_COUNT>, Reply);
    type Error = MgmtError;
    type GraphSpec = MgmtLeaseSpec<T, U, C, E>;

    fn run<'lease, 'lease_cfg>(
        _lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        _seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        ControlStep::Abort(MgmtError::InvalidTransition)
    }

    fn run_with_graph<'lease, 'lease_cfg, 'graph>(
        graph: &'graph mut LeaseGraph<'graph, MgmtLeaseSpec<T, U, C, E>>,
        root_lease: &mut RendezvousLease<'lease, 'lease_cfg, T, U, C, E, Self::Spec>,
        mut seed: Self::Seed,
    ) -> ControlStep<Self::Output, Self::Error>
    where
        'lease_cfg: 'lease,
    {
        let reply = match seed.request {
            RequestAction::Load { slot, mode } => {
                if let Err(err) = root_lease.with_rendezvous(|rv| {
                    let facet = rv.slot_facet();
                    facet.load_commit(rv, slot, &mut seed.manager)
                }) {
                    return ControlStep::Abort(err);
                }

                if let Some(result) = {
                    let mut handle = graph.root_handle_mut();
                    handle
                        .context()
                        .slots_mut()
                        .map(|slots| slots.track_stage(slot))
                } && result.is_err()
                {
                    return ControlStep::Abort(MgmtError::InvalidTransition);
                }

                match mode {
                    LoadMode::Stage => {
                        let staged_version = match seed.manager.staged_version(slot) {
                            Some(version) => version,
                            None => return ControlStep::Abort(MgmtError::NoStagedImage),
                        };
                        Reply::Loaded(LoadReport { staged_version })
                    }
                    LoadMode::Activate => match root_lease.with_rendezvous(|rv| {
                        let facet = rv.slot_facet();
                        let scheduled = facet.schedule_activate(rv, slot, &mut seed.manager)?;
                        let committed =
                            facet.on_decision_boundary_for_slot(rv, slot, &mut seed.manager)?;
                        Ok(committed.unwrap_or(scheduled))
                    }) {
                        Ok(report) => Reply::ActivationScheduled(report),
                        Err(err) => return ControlStep::Abort(err),
                    },
                }
            }
            RequestAction::Activate { slot } => match root_lease.with_rendezvous(|rv| {
                let facet = rv.slot_facet();
                let scheduled = facet.schedule_activate(rv, slot, &mut seed.manager)?;
                let committed = facet.on_decision_boundary_for_slot(rv, slot, &mut seed.manager)?;
                Ok(committed.unwrap_or(scheduled))
            }) {
                Ok(report) => Reply::ActivationScheduled(report),
                Err(err) => return ControlStep::Abort(err),
            },
            RequestAction::Revert { slot } => match root_lease.with_rendezvous(|rv| {
                let facet = rv.slot_facet();
                facet.revert(rv, slot, &mut seed.manager)
            }) {
                Ok(report) => Reply::Reverted(report),
                Err(err) => return ControlStep::Abort(err),
            },
            RequestAction::Stats { slot } => match seed.manager.stats(slot) {
                Ok(stats) => Reply::Stats {
                    stats,
                    staged_version: seed.manager.staged_version(slot),
                },
                Err(err) => return ControlStep::Abort(err),
            },
        };
        ControlStep::Complete((seed.manager, reply))
    }
}

fn slot_id(slot: Slot) -> u32 {
    match slot {
        Slot::Forward => 0,
        Slot::EndpointRx => 1,
        Slot::EndpointTx => 2,
        Slot::Rendezvous => 3,
        Slot::Route => 4,
    }
}

fn decode_slot(slot: u8) -> Result<Slot, CodecError> {
    match slot {
        0 => Ok(Slot::Forward),
        1 => Ok(Slot::EndpointRx),
        2 => Ok(Slot::EndpointTx),
        3 => Ok(Slot::Rendezvous),
        4 => Ok(Slot::Route),
        _ => Err(CodecError::Invalid("unknown management slot")),
    }
}

// ======== Slot inventory =====================================================

#[derive(Clone, Copy, Debug)]
struct ImageMeta {
    version: u32,
    header: Header,
}

impl ImageMeta {
    fn new(version: u32, header: Header) -> Self {
        Self { version, header }
    }

    fn code_len(&self) -> usize {
        self.header.code_len as usize
    }
}

#[derive(Clone, Copy, Debug)]
struct StageEntry(ImageMeta);

impl StageEntry {
    fn meta(&self) -> ImageMeta {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveEntry {
    current: ImageMeta,
    previous: Option<ImageMeta>,
}

impl ActiveEntry {
    fn new(current: ImageMeta) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    fn install_previous(&mut self, previous: ImageMeta) {
        self.previous = Some(previous);
    }

    fn current(&self) -> ImageMeta {
        self.current
    }

    fn revert(&mut self) -> Result<ImageMeta, MgmtError> {
        let previous = self.previous.ok_or(MgmtError::NoPreviousImage)?;
        self.previous = Some(self.current);
        self.current = previous;
        Ok(self.current)
    }
}

#[derive(Clone, Copy, Debug)]
struct SlotInventory {
    staged: Option<StageEntry>,
    active: Option<ActiveEntry>,
}

impl SlotInventory {
    fn new() -> Self {
        Self {
            staged: None,
            active: None,
        }
    }

    fn stage(&mut self, meta: ImageMeta) {
        self.staged = Some(StageEntry(meta));
    }

    fn take_stage(&mut self) -> Result<StageEntry, MgmtError> {
        self.staged.take().ok_or(MgmtError::NoStagedImage)
    }

    fn current_active(&self) -> Option<ImageMeta> {
        self.active.as_ref().map(ActiveEntry::current)
    }

    fn staged(&self) -> Option<ImageMeta> {
        self.staged.as_ref().map(StageEntry::meta)
    }

    fn install_active(&mut self, current: ImageMeta, previous: Option<ImageMeta>) {
        let mut entry = ActiveEntry::new(current);
        if let Some(prev) = previous {
            entry.install_previous(prev);
        }
        self.active = Some(entry);
    }

    fn active_mut(&mut self) -> Result<&mut ActiveEntry, MgmtError> {
        self.active.as_mut().ok_or(MgmtError::NoActiveImage)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PendingVersion(Option<u32>);

impl PendingVersion {
    fn begin(&mut self, version: u32) {
        self.0 = Some(version);
    }

    fn take(&mut self) -> Result<u32, MgmtError> {
        self.0.take().ok_or(MgmtError::LoaderNotFinalised)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        epf::{host::HostSlots, ops, verifier::compute_hash},
        rendezvous::slots::SlotStorage,
    };

    fn stage_image(
        manager: &mut Manager<AwaitBegin, { SLOT_COUNT }>,
        slot: Slot,
        storage: &mut SlotStorage,
        code: &[u8],
    ) -> u32 {
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 64,
            flags: 0,
            hash: compute_hash(code),
        };
        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, code).unwrap();
        manager.load_commit(slot, storage).unwrap()
    }

    #[test]
    fn policy_switch_commits_only_on_decision_boundary() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let code = [0x01u8, 0x02, 0x03, 0x04];
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 64,
            flags: 0,
            hash: compute_hash(&code),
        };

        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, &code[..2]).unwrap();
        manager.load_chunk(slot, 2, &code[2..]).unwrap();

        let mut storage = SlotStorage::new();
        manager.load_commit(slot, &mut storage).unwrap();

        let mut host_slots = HostSlots::new();
        let scheduled = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled.version, 1);
        assert_eq!(host_slots.active_digest(slot), 0);

        let report = manager
            .on_decision_boundary(slot, &mut storage, &mut host_slots)
            .unwrap()
            .expect("decision boundary should commit scheduled activation");
        assert_eq!(report.version, 1);
        assert_ne!(host_slots.active_digest(slot), 0);
    }

    #[test]
    fn chunk_out_of_order_is_rejected() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let header = Header {
            code_len: 2,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(&[0xAA, 0xBB]),
        };
        manager.load_begin(slot, header).unwrap();

        let err = manager
            .load_chunk(slot, 1, &[0xAA, 0xBB])
            .expect_err("chunk should be rejected");
        assert!(matches!(
            err,
            MgmtError::ChunkOutOfOrder {
                expected: 0,
                got: 1
            }
        ));
    }

    #[test]
    fn load_commit_rejects_get_input_for_forward_slot() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Forward;
        let code = [
            crate::epf::ops::instr::GET_INPUT,
            0x00,
            0x00,
            crate::epf::ops::instr::HALT,
        ];
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(&code),
        };

        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, &code).unwrap();

        let mut storage = SlotStorage::new();
        let err = manager.load_commit(slot, &mut storage).unwrap_err();
        assert!(matches!(err, MgmtError::LoaderNotFinalised));
    }

    #[test]
    fn set_policy_mode_updates_live_host_slots_immediately() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let host_slots = HostSlots::new();

        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Shadow);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Enforce);

        manager
            .set_policy_mode(slot, PolicyMode::Enforce, &host_slots)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Enforce);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Enforce);

        manager
            .set_policy_mode(slot, PolicyMode::Shadow, &host_slots)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Shadow);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);
    }

    #[test]
    fn set_policy_mode_staged_does_not_touch_live_host_slots() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let host_slots = HostSlots::new();

        manager
            .set_policy_mode(slot, PolicyMode::Shadow, &host_slots)
            .unwrap();
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);

        manager
            .set_policy_mode_staged(slot, PolicyMode::Enforce)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Enforce);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);
    }

    #[test]
    fn promotion_gate_requires_consecutive_windows_and_resets_on_failure() {
        let mut gate = PromotionGateState::default();
        let thresholds = PromotionGateThresholds {
            min_samples: 3,
            max_divergence_ppm: 10,
            max_reject_delta_ppm: 10,
            max_p99_eval_us: 100,
            max_latency_increase_ppm: 10,
            max_fail_closed_ppm: 10,
            required_consecutive_windows: 2,
        };
        let pass = PromotionGateWindow {
            sample_count: 3,
            divergence_ppm: 5,
            reject_delta_ppm: 5,
            p99_eval_us: 50,
            latency_increase_ppm: 5,
            fail_closed_ppm: 5,
        };
        let fail = PromotionGateWindow {
            sample_count: 3,
            divergence_ppm: 11,
            reject_delta_ppm: 5,
            p99_eval_us: 50,
            latency_increase_ppm: 5,
            fail_closed_ppm: 5,
        };

        assert!(!gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 1);
        assert!(gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 2);

        assert!(!gate.observe(fail, thresholds));
        assert_eq!(gate.consecutive_windows, 0);
        assert!(!gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 1);
    }

    #[test]
    fn manager_observe_promotion_window_tracks_slot_gate_state() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let thresholds = PromotionGateThresholds {
            min_samples: 2,
            max_divergence_ppm: 10,
            max_reject_delta_ppm: 10,
            max_p99_eval_us: 100,
            max_latency_increase_ppm: 10,
            max_fail_closed_ppm: 10,
            required_consecutive_windows: 2,
        };
        let pass = PromotionGateWindow {
            sample_count: 2,
            divergence_ppm: 0,
            reject_delta_ppm: 0,
            p99_eval_us: 10,
            latency_increase_ppm: 0,
            fail_closed_ppm: 0,
        };
        let fail = PromotionGateWindow {
            sample_count: 2,
            divergence_ppm: 20,
            reject_delta_ppm: 0,
            p99_eval_us: 10,
            latency_increase_ppm: 0,
            fail_closed_ppm: 0,
        };

        assert!(
            !manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
        assert!(
            manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
        assert!(
            !manager
                .observe_promotion_window(Slot::Forward, fail, thresholds)
                .unwrap()
        );
        assert!(
            manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
    }

    #[test]
    fn schedule_activate_overwrites_pending_epoch() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        let v1 = stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        let scheduled_v1 = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled_v1.version, v1);

        let v2 = stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x01, 0x00],
        );
        let scheduled_v2 = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled_v2.version, v2);
        assert_eq!(
            manager.slot_states[slot_index(slot)].pending_epoch,
            Some(v2),
            "latest schedule must overwrite pending epoch"
        );
    }

    #[test]
    fn revert_clears_pending_epoch() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v1 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x02, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v2 activation");
            let _ = report;
        }

        let v3 = stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x03, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        assert_eq!(
            manager.slot_states[slot_index(slot)].pending_epoch,
            Some(v3)
        );

        let mut host_slots = HostSlots::new();
        manager.revert(slot, &mut storage, &mut host_slots).unwrap();
        assert_eq!(manager.slot_states[slot_index(slot)].pending_epoch, None);
    }

    #[test]
    fn schedule_then_revert_does_not_activate_stale_pending() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v1 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x02, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v2 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x03, 0x00],
        );
        manager.schedule_activate(slot).unwrap();

        {
            let mut host_slots = HostSlots::new();
            manager.revert(slot, &mut storage, &mut host_slots).unwrap();
        }
        let mut host_slots = HostSlots::new();
        let boundary = manager
            .on_decision_boundary(slot, &mut storage, &mut host_slots)
            .unwrap();
        assert!(
            boundary.is_none(),
            "stale pending must not activate after revert"
        );
    }
}
