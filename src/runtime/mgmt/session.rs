use core::convert::TryFrom;

use crate::{
    control::cap::{
        AllowsCanonical, GenericCapToken, MintConfigMarker,
        resource_kinds::{LoadBeginKind, LoadCommitKind, LoopBreakKind, LoopContinueKind},
    },
    endpoint::{ControlOutcome, RecvError, SendError, cursor::CursorEndpoint},
    epf::verifier::Header,
    g::steps::{self, StepConcat, StepCons, StepNil},
    g::{self, Program},
    global::const_dsl::{DynamicMeta, HandlePlan},
    observe::{TapBatch, TapEvent, WaitForNewUserEvents},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_MGMT_COMMAND, LABEL_MGMT_LOAD_BEGIN,
        LABEL_MGMT_LOAD_BEGIN_DATA, LABEL_MGMT_LOAD_CHUNK, LABEL_MGMT_LOAD_COMMIT,
        LABEL_OBSERVE_BATCH, LABEL_OBSERVE_EVENT, LABEL_OBSERVE_STREAM_END,
        LABEL_OBSERVE_SUBSCRIBE,
    },
    transport::wire::{CodecError, WireEncode},
};

use super::{
    Activate, AwaitBegin, Command, LoadBegin, LoadChunk, MgmtError, MgmtLinks, MgmtSeed, Revert,
    StatsReq, SubscribeReq,
};

pub type Controller = g::Role<0>;
pub type Cluster = g::Role<1>;

pub const ROLE_CONTROLLER: u8 = 0;
pub const ROLE_CLUSTER: u8 = 1;

pub type LoadBeginTokenMsg = g::Msg<
    LABEL_MGMT_LOAD_BEGIN,
    GenericCapToken<LoadBeginKind>,
    g::ExternalControl<LoadBeginKind>,
>;
pub type LoadBeginMsg = g::Msg<LABEL_MGMT_LOAD_BEGIN_DATA, LoadBegin>;
pub type LoadChunkMsg = g::Msg<LABEL_MGMT_LOAD_CHUNK, LoadChunk>;
pub type LoadCommitTokenMsg = g::Msg<
    LABEL_MGMT_LOAD_COMMIT,
    GenericCapToken<LoadCommitKind>,
    g::ExternalControl<LoadCommitKind>,
>;
pub type LoopContinueMsg = g::Msg<
    LABEL_LOOP_CONTINUE,
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
pub type LoopBreakMsg =
    g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, g::CanonicalControl<LoopBreakKind>>;
pub type CommandMsg = g::Msg<LABEL_MGMT_COMMAND, CommandEnvelope>;

// Streaming observe message types
pub type SubscribeMsg = g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>;
pub type TapEventMsg = g::Msg<LABEL_OBSERVE_EVENT, TapEvent>;
pub type TapBatchMsg = g::Msg<LABEL_OBSERVE_BATCH, TapBatch>;
pub type StreamContinueMsg = g::Msg<
    LABEL_LOOP_CONTINUE,
    GenericCapToken<LoopContinueKind>,
    g::CanonicalControl<LoopContinueKind>,
>;
pub type StreamBreakMsg = g::Msg<
    LABEL_LOOP_BREAK,
    GenericCapToken<LoopBreakKind>,
    g::CanonicalControl<LoopBreakKind>,
>;

/// End-of-stream marker for the streaming observe protocol.
/// Sent by Cluster on Break arm to signal Controller that the stream has ended.
/// This allows Controller to distinguish Continue (TapEvent) from Break (StreamEnd)
/// without needing to parse payload sentinel values.
pub type StreamEndMsg = g::Msg<LABEL_OBSERVE_STREAM_END, ()>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandEnvelope {
    Activate(Activate),
    Revert(Revert),
    Stats(StatsReq),
}

impl WireEncode for CommandEnvelope {
    fn encoded_len(&self) -> Option<usize> {
        let payload_len = match self {
            CommandEnvelope::Activate(payload) => payload.encoded_len().unwrap_or(1),
            CommandEnvelope::Revert(payload) => payload.encoded_len().unwrap_or(1),
            CommandEnvelope::Stats(payload) => payload.encoded_len().unwrap_or(1),
        };
        Some(1 + payload_len)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        let written = match self {
            CommandEnvelope::Activate(payload) => {
                out[0] = 0;
                payload.encode_into(&mut out[1..])?
            }
            CommandEnvelope::Revert(payload) => {
                out[0] = 1;
                payload.encode_into(&mut out[1..])?
            }
            CommandEnvelope::Stats(payload) => {
                out[0] = 2;
                payload.encode_into(&mut out[1..])?
            }
        };
        Ok(written + 1)
    }
}

impl<'a> crate::transport::wire::WireDecode<'a> for CommandEnvelope {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.is_empty() {
            return Err(CodecError::Truncated);
        }
        let tag = input[0];
        let payload = &input[1..];
        match tag {
            0 => Ok(CommandEnvelope::Activate(Activate::decode_from(payload)?)),
            1 => Ok(CommandEnvelope::Revert(Revert::decode_from(payload)?)),
            2 => Ok(CommandEnvelope::Stats(StatsReq::decode_from(payload)?)),
            _ => Err(CodecError::Invalid("unknown management command tag")),
        }
    }
}

type LoadBeginSteps = StepCons<
    steps::SendStep<Controller, Cluster, LoadBeginTokenMsg>,
    StepCons<steps::SendStep<Controller, Cluster, LoadBeginMsg>, StepNil>,
>;

type LoopChunkStep = StepCons<steps::SendStep<Controller, Cluster, LoadChunkMsg>, StepNil>;
// CanonicalControl requires self-send (Controller → Controller)
type LoopContinueStep = StepCons<steps::SendStep<Controller, Controller, LoopContinueMsg>, StepNil>;
type LoopBreakStep = StepCons<steps::SendStep<Controller, Controller, LoopBreakMsg>, StepNil>;
type LoopRouteSteps = <LoopContinueStep as StepConcat<LoopBreakStep>>::Output;
type LoopSegmentSteps = <LoopChunkStep as StepConcat<LoopRouteSteps>>::Output;

type LoadCommitSteps = StepCons<steps::SendStep<Controller, Cluster, LoadCommitTokenMsg>, StepNil>;

type CommandStep = StepCons<steps::SendStep<Controller, Cluster, CommandMsg>, StepNil>;
type CommandRouteSteps = CommandStep;

type AfterLoop = <LoadBeginSteps as StepConcat<LoopSegmentSteps>>::Output;
type AfterCommit = <AfterLoop as StepConcat<LoadCommitSteps>>::Output;
type FullProgramSteps = <AfterCommit as StepConcat<CommandRouteSteps>>::Output;

type ControllerLocal = g::LocalProgram<Controller, FullProgramSteps>;
type ClusterLocal = g::LocalProgram<Cluster, FullProgramSteps>;

// CanonicalControl requires self-send (Controller → Controller)
const LOOP_CONTINUE_ARM: Program<LoopContinueStep> = g::with_control_plan(
    g::send::<Controller, Controller, LoopContinueMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_META),
);
const LOOP_BREAK_ARM: Program<LoopBreakStep> = g::with_control_plan(
    g::send::<Controller, Controller, LoopBreakMsg, 0>(),
    HandlePlan::dynamic(LOOP_POLICY_ID, LOOP_META),
);

const LOOP_POLICY_ID: u16 = 700;
const LOOP_META: DynamicMeta = DynamicMeta::new();

const LOAD_BEGIN: Program<LoadBeginSteps> = g::seq(
    g::send::<Controller, Cluster, LoadBeginTokenMsg, 0>(),
    g::send::<Controller, Cluster, LoadBeginMsg, 0>(),
);

const LOOP_SEGMENT: Program<LoopSegmentSteps> = g::seq(
    g::send::<Controller, Cluster, LoadChunkMsg, 0>(),
    // Self-send route: Controller → Controller for CanonicalControl
    g::route::<ROLE_CONTROLLER, _>(
        g::route_chain::<ROLE_CONTROLLER, _>(LOOP_CONTINUE_ARM)
            .and::<LoopBreakStep>(LOOP_BREAK_ARM),
    ),
);

const LOAD_COMMIT: Program<LoadCommitSteps> = g::send::<Controller, Cluster, LoadCommitTokenMsg, 0>();

const COMMAND_ROUTE: Program<CommandRouteSteps> = g::send::<Controller, Cluster, CommandMsg, 0>();

pub const PROGRAM: Program<FullProgramSteps> = LOAD_BEGIN
    .then(LOOP_SEGMENT)
    .then(LOAD_COMMIT)
    .then(COMMAND_ROUTE);

pub static CONTROLLER_PROGRAM: g::RoleProgram<'static, ROLE_CONTROLLER, ControllerLocal> =
    g::project::<ROLE_CONTROLLER, FullProgramSteps, _>(&PROGRAM);
pub static CLUSTER_PROGRAM: g::RoleProgram<'static, ROLE_CLUSTER, ClusterLocal> =
    g::project::<ROLE_CLUSTER, FullProgramSteps, _>(&PROGRAM);

const _: () = crate::control::lease::planner::assert_program_covers_facets(
    &CONTROLLER_PROGRAM,
    super::MGMT_FACET_NEEDS,
);

const _: () = crate::control::lease::planner::assert_program_covers_facets(
    &CLUSTER_PROGRAM,
    super::MGMT_FACET_NEEDS,
);

fn map_recv_error(_err: RecvError) -> MgmtError {
    MgmtError::InvalidTransition
}

fn map_send_error(_err: SendError) -> MgmtError {
    MgmtError::InvalidTransition
}

/// Drive the cluster role of the management session up to the command dispatch.
pub async fn drive_cluster<'lease, T, U, C, Mint, const MAX_RV: usize>(
    mut manager: super::Manager<super::AwaitBegin, { super::SLOT_COUNT }>,
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::EpochInit,
        MAX_RV,
        Mint,
    >,
) -> Result<
    (
        CursorEndpoint<'lease, ROLE_CLUSTER, T, U, C, crate::control::cap::EpochInit, MAX_RV, Mint>,
        MgmtSeed<AwaitBegin>,
    ),
    (
        MgmtError,
        super::Manager<super::AwaitBegin, { super::SLOT_COUNT }>,
    ),
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
{
    let recv_result = endpoint.recv::<LoadBeginTokenMsg>().await;
    let (next_endpoint, _) = match recv_result {
        Ok(value) => value,
        Err(err) => return Err((map_recv_error(err), manager)),
    };
    let recv_result = next_endpoint.recv::<LoadBeginMsg>().await;
    let (mut endpoint, begin) = match recv_result {
        Ok(value) => value,
        Err(err) => return Err((map_recv_error(err), manager)),
    };

    let slot = match super::slot_from_u8(begin.slot) {
        Some(slot) => slot,
        None => return Err((MgmtError::InvalidSlot(begin.slot), manager)),
    };

    let code_len = match u16::try_from(begin.code_len) {
        Ok(val) => val,
        Err(_) => {
            return Err((
                MgmtError::ChunkTooLarge {
                    remaining: 0,
                    provided: begin.code_len,
                },
                manager,
            ));
        }
    };

    let header = Header {
        code_len,
        fuel_max: begin.fuel_max,
        mem_len: begin.mem_len,
        flags: 0,
        hash: begin.hash,
    };

    if let Err(err) = manager.load_begin(slot, header) {
        return Err((err, manager));
    }

    // Two-layer protocol design:
    // - Controller sends CanonicalControl for internal state transitions (self-send)
    // - Cluster uses chunk.is_last to determine loop termination (payload data)
    // - No offer() needed - the route is invisible to Cluster's projection
    loop {
        let recv_result = endpoint.recv::<LoadChunkMsg>().await;
        let (after_chunk, chunk) = match recv_result {
            Ok(value) => value,
            Err(err) => return Err((map_recv_error(err), manager)),
        };

        let len = usize::from(chunk.len);
        if len > super::LOAD_CHUNK_MAX {
            return Err((
                MgmtError::ChunkTooLarge {
                    remaining: 0,
                    provided: chunk.len as u32,
                },
                manager,
            ));
        }

        if let Err(err) = manager.load_chunk(slot, chunk.offset, &chunk.bytes[..len]) {
            return Err((err, manager));
        }

        endpoint = after_chunk;

        // Use is_last flag from payload instead of route control
        if chunk.is_last {
            break;
        }
    }

    let recv_result = endpoint.recv::<LoadCommitTokenMsg>().await;
    let (endpoint, _) = match recv_result {
        Ok(value) => value,
        Err(err) => return Err((map_recv_error(err), manager)),
    };
    let recv_result = endpoint.recv::<CommandMsg>().await;
    let (endpoint, envelope) = match recv_result {
        Ok(value) => value,
        Err(err) => return Err((map_recv_error(err), manager)),
    };
    let command = match envelope {
        CommandEnvelope::Activate(msg) => {
            let slot = match super::slot_from_u8(msg.slot) {
                Some(slot) => slot,
                None => return Err((MgmtError::InvalidSlot(msg.slot), manager)),
            };
            Command::Activate { slot }
        }
        CommandEnvelope::Revert(msg) => {
            let slot = match super::slot_from_u8(msg.slot) {
                Some(slot) => slot,
                None => return Err((MgmtError::InvalidSlot(msg.slot), manager)),
            };
            Command::Revert { slot }
        }
        CommandEnvelope::Stats(msg) => {
            let slot = match super::slot_from_u8(msg.slot) {
                Some(slot) => slot,
                None => return Err((MgmtError::InvalidSlot(msg.slot), manager)),
            };
            Command::Stats { slot }
        }
    };

    let seed = MgmtSeed {
        load_slot: slot,
        command,
        manager,
        links: MgmtLinks::new(),
    };

    Ok((endpoint, seed))
}

/// Controller-side plan for driving a management session.
pub struct ControllerPlan<'chunks> {
    pub load_token: GenericCapToken<LoadBeginKind>,
    pub load_begin: LoadBegin,
    pub chunks: &'chunks [LoadChunk],
    pub commit_token: GenericCapToken<LoadCommitKind>,
    pub command: Command,
}

/// Drive the controller role of the management session according to `plan`.
pub async fn drive_controller<'lease, T, U, C, Mint, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::EpochInit,
        MAX_RV,
        Mint,
    >,
    plan: ControllerPlan<'_>,
) -> Result<
    CursorEndpoint<'lease, ROLE_CONTROLLER, T, U, C, crate::control::cap::EpochInit, MAX_RV, Mint>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
{
    if plan.chunks.is_empty() {
        return Err(MgmtError::InvalidTransition);
    }

    let (endpoint_after_token, _) = endpoint
        .flow::<LoadBeginTokenMsg>()
        .map_err(|err| map_send_error(err.into()))?
        .send(&plan.load_token)
        .await
        .map_err(map_send_error)?;
    let (mut endpoint, _) = endpoint_after_token
        .flow::<LoadBeginMsg>()
        .map_err(|err| map_send_error(err.into()))?
        .send(&plan.load_begin)
        .await
        .map_err(map_send_error)?;

    for (idx, chunk) in plan.chunks.iter().enumerate() {
        let is_last = idx + 1 == plan.chunks.len();

        // Set is_last flag on chunk to inform Cluster of loop termination
        let chunk_with_flag = super::LoadChunk {
            offset: chunk.offset,
            len: chunk.len,
            is_last,
            bytes: chunk.bytes,
        };

        let (after_chunk, _) = endpoint
            .flow::<LoadChunkMsg>()
            .map_err(|err| map_send_error(err.into()))?
            .send(&chunk_with_flag)
            .await
            .map_err(map_send_error)?;

        // Self-send CanonicalControl uses flow().send(()) - wire transmission skipped for self-send
        endpoint = if is_last {
            let flow = after_chunk
                .flow::<LoopBreakMsg>()
                .map_err(|err| map_send_error(err.into()))?;
            let (next_endpoint, outcome) = flow
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(matches!(outcome, ControlOutcome::Canonical(_)));
            next_endpoint
        } else {
            let (next_endpoint, outcome) = after_chunk
                .flow::<LoopContinueMsg>()
                .map_err(|err| map_send_error(err.into()))?
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(matches!(outcome, ControlOutcome::Canonical(_)));
            next_endpoint
        };
    }

    let (endpoint, _) = endpoint
        .flow::<LoadCommitTokenMsg>()
        .map_err(|err| map_send_error(err.into()))?
        .send(&plan.commit_token)
        .await
        .map_err(map_send_error)?;

    let slot_to_u8 = |slot: super::Slot| super::slot_id(slot) as u8;

    let envelope = match plan.command {
        Command::Activate { slot } => CommandEnvelope::Activate(Activate {
            slot: slot_to_u8(slot),
        }),
        Command::Revert { slot } => CommandEnvelope::Revert(Revert {
            slot: slot_to_u8(slot),
        }),
        Command::Stats { slot } => CommandEnvelope::Stats(StatsReq {
            slot: slot_to_u8(slot),
        }),
    };
    let (endpoint, _) = endpoint
        .flow::<CommandMsg>()
        .map_err(|err| map_send_error(err.into()))?
        .send(&envelope)
        .await
        .map_err(map_send_error)?;

    Ok(endpoint)
}

// ============================================================================
// Streaming Observe Choreography
// ============================================================================
//
// Real-time event streaming: Controller subscribes, Cluster streams TapEvents.
//
// Design: Use the same pattern as hibana-quic STREAM_LOOP.
// Cluster is the sender AND loop controller (sender = loop controller pattern).
//
// Loop structure (with .linger() for typestate loop semantics):
//   Subscribe → route.linger(
//     arm0: Continue → TapEvent → (rewind via linger)
//     arm1: Break → exit
//   )
//
// Key insight: .linger() preserves loop structure in projections, allowing both
// roles to iterate their typestate properly.
//
// Projections:
//   - Controller: Send(Subscribe) → route.linger([Recv(TapEvent)]* | exit)
//   - Cluster: Recv(Subscribe) → route.linger([LocalAction(Continue) → Send(TapEvent)]* | LocalAction(Break))
//
// Driver behavior:
//   - Cluster: offer(Continue → send TapEvent | Break → send StreamEnd)
//   - Controller: offer() to distinguish TapEvent (arm0) vs StreamEnd (arm1)
//
// StreamEndMsg (label 39) signals end-of-stream instead of TapEvent sentinel.

type StreamSubscribeStep = StepCons<steps::SendStep<Controller, Cluster, SubscribeMsg>, StepNil>;
type StreamBatchStep = StepCons<steps::SendStep<Cluster, Controller, TapBatchMsg>, StepNil>;
type StreamEndStep = StepCons<steps::SendStep<Cluster, Controller, StreamEndMsg>, StepNil>;

// Stream loop control messages (Cluster self-send for CanonicalControl)
type StreamLoopContinueStep = StepCons<steps::SendStep<Cluster, Cluster, StreamContinueMsg>, StepNil>;
type StreamLoopBreakStep = StepCons<steps::SendStep<Cluster, Cluster, StreamBreakMsg>, StepNil>;

// Stream loop arms:
// - Continue arm: self-send Continue → TapBatch (loops via linger)
// - Break arm: self-send Break → StreamEnd (exits loop)
// Different message types allow Controller to distinguish via offer().
type StreamLoopContinueSteps = <StreamLoopContinueStep as StepConcat<StreamBatchStep>>::Output;
type StreamLoopBreakSteps = <StreamLoopBreakStep as StepConcat<StreamEndStep>>::Output;
type StreamLoopRouteSteps = <StreamLoopContinueSteps as StepConcat<StreamLoopBreakSteps>>::Output;

// Full program: Subscribe → route.linger(Continue → TapBatch | Break)
type StreamProgramSteps = <StreamSubscribeStep as StepConcat<StreamLoopRouteSteps>>::Output;

type StreamControllerLocal = g::LocalProgram<Controller, StreamProgramSteps>;
type StreamClusterLocal = g::LocalProgram<Cluster, StreamProgramSteps>;

const STREAM_SUBSCRIBE: Program<StreamSubscribeStep> =
    g::send::<Controller, Cluster, SubscribeMsg, 0>();

// Stream loop arms (Cluster self-send for CanonicalControl)
// No HandlePlan::dynamic needed - self-send skips resolver validation entirely.
const STREAM_LOOP_CONTINUE_ARM: Program<StreamLoopContinueSteps> =
    g::send::<Cluster, Cluster, StreamContinueMsg, 0>()
        .then(g::send::<Cluster, Controller, TapBatchMsg, 0>());

// Break arm sends StreamEndMsg to signal end-of-stream (distinct label from TapEventMsg)
const STREAM_LOOP_BREAK_ARM: Program<StreamLoopBreakSteps> =
    g::send::<Cluster, Cluster, StreamBreakMsg, 0>()
        .then(g::send::<Cluster, Controller, StreamEndMsg, 0>());

// Route is local to Cluster (1 → 1) with .linger() for loop semantics
const STREAM_LOOP_ROUTE: Program<StreamLoopRouteSteps> = g::route::<ROLE_CLUSTER, _>(
    g::route_chain::<ROLE_CLUSTER, StreamLoopContinueSteps>(STREAM_LOOP_CONTINUE_ARM)
        .linger()
        .and::<StreamLoopBreakSteps>(STREAM_LOOP_BREAK_ARM),
);

pub const STREAM_PROGRAM: Program<StreamProgramSteps> = STREAM_SUBSCRIBE.then(STREAM_LOOP_ROUTE);

pub static STREAM_CONTROLLER_PROGRAM: g::RoleProgram<'static, ROLE_CONTROLLER, StreamControllerLocal> =
    g::project::<ROLE_CONTROLLER, StreamProgramSteps, _>(&STREAM_PROGRAM);
pub static STREAM_CLUSTER_PROGRAM: g::RoleProgram<'static, ROLE_CLUSTER, StreamClusterLocal> =
    g::project::<ROLE_CLUSTER, StreamProgramSteps, _>(&STREAM_PROGRAM);

/// Callback for stream termination decision.
pub trait StreamControl {
    /// Returns true to continue streaming, false to break.
    fn should_continue(&mut self) -> bool;
}

/// Drive the cluster role of the streaming observe session.
///
/// Streams TapBatches to Controller in real-time as EPF TAP_OUT fires.
/// Uses Greedy Batch mode: fills batch from ring buffer before sending.
///
/// Flow:
/// 1. Receive Subscribe from Controller
/// 2. Loop: wait for events → fill batch → self-send Continue → send TapBatch
/// 3. When control.should_continue() returns false, self-send Break and exit
///
/// Uses proper hibana loop semantics with .linger() for typestate rewind.
pub async fn drive_stream_cluster<'lease, T, U, C, Mint, Ctrl, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::EpochInit,
        MAX_RV,
        Mint,
        B,
    >,
    mut control: Ctrl,
) -> Result<
    CursorEndpoint<'lease, ROLE_CLUSTER, T, U, C, crate::control::cap::EpochInit, MAX_RV, Mint, B>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    Ctrl: StreamControl,
    B: crate::binding::BindingSlot,
{
    // Receive subscribe request
    let (mut endpoint, _subscribe) = endpoint
        .recv::<SubscribeMsg>()
        .await
        .map_err(map_recv_error)?;

    let mut cursor: usize = crate::observe::user_head().unwrap_or(0);

    // Stream loop: Continue → TapBatch | Break
    // Cluster is the loop controller (sender of self-send Continue/Break)
    // Greedy Batch: fill batch from ring buffer in single iteration
    loop {
        // Wait for at least one new event
        let new_head = WaitForNewUserEvents::new(cursor).await;

        // Check if we should continue streaming
        if !control.should_continue() {
            // Break arm: self-send Break, then send StreamEndMsg
            let (ep, outcome) = endpoint
                .flow::<StreamBreakMsg>()
                .map_err(|err| map_send_error(err.into()))?
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(matches!(outcome, ControlOutcome::Canonical(_)));

            // Send StreamEndMsg to signal end-of-stream
            let (ep, _) = ep
                .flow::<StreamEndMsg>()
                .map_err(|err| map_send_error(err.into()))?
                .send(&())
                .await
                .map_err(map_send_error)?;

            endpoint = ep;
            break;
        }

        // Greedy drain: fill batch from ring buffer
        let mut batch = TapBatch::empty();

        // Check for ring overrun (cursor fell behind head by more than RING_SIZE)
        let ring_size = crate::runtime::consts::RING_BUFFER_SIZE;
        if new_head > cursor + ring_size {
            batch.set_lost_events((new_head - cursor - ring_size) as u32);
            cursor = new_head - ring_size; // Skip to oldest available
        }

        while cursor < new_head && !batch.is_full() {
            if let Some(event) = crate::observe::read_user_at(cursor) {
                batch.push(event);
            }
            cursor += 1;
        }

        if batch.len() > 0 {
            // Continue arm: self-send Continue, then send TapBatch
            let (ep, outcome) = endpoint
                .flow::<StreamContinueMsg>()
                .map_err(|err| map_send_error(err.into()))?
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(matches!(outcome, ControlOutcome::Canonical(_)));

            // Send TapBatch to Controller
            let (ep, _) = ep
                .flow::<TapBatchMsg>()
                .map_err(|err| map_send_error(err.into()))?
                .send(&batch)
                .await
                .map_err(map_send_error)?;

            endpoint = ep;
        }
    }

    Ok(endpoint)
}

/// Drive the controller role of the streaming observe session.
///
/// Receives TapBatches from Cluster until the stream ends (Break arm).
/// Processes each event in the batch via the callback.
///
/// Flow:
/// 1. Send Subscribe to Cluster
/// 2. Loop: offer() to get route arm, decode TapBatch if Continue arm
/// 3. When Break arm is selected, exit loop
///
/// Uses proper hibana loop semantics with offer()/decode() for route handling.
/// RouteTable propagates Cluster's self-send decisions to Controller.
pub async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::EpochInit,
        MAX_RV,
        Mint,
        B,
    >,
    subscribe: SubscribeReq,
    mut on_event: F,
) -> Result<
    CursorEndpoint<'lease, ROLE_CONTROLLER, T, U, C, crate::control::cap::EpochInit, MAX_RV, Mint, B>,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    F: FnMut(TapEvent) -> bool,
    B: crate::binding::BindingSlot,
{
    // Send subscribe request
    let (mut endpoint, _) = endpoint
        .flow::<SubscribeMsg>()
        .map_err(|err| map_send_error(err.into()))?
        .send(&subscribe)
        .await
        .map_err(map_send_error)?;

    // Stream loop: use offer() to distinguish TapBatch (Continue arm) from StreamEnd (Break arm).
    // Controller receives route decision via RouteTable from Cluster's self-send.
    loop {
        let branch = endpoint.offer().await.map_err(map_recv_error)?;

        let label = branch.label();

        // Check label to determine which arm we're in
        if label == LABEL_OBSERVE_STREAM_END {
            // Break arm: StreamEndMsg received, exit loop
            let (ep, ()) = branch.decode::<StreamEndMsg>().await.map_err(map_recv_error)?;
            return Ok(ep);
        } else if label == LABEL_OBSERVE_BATCH {
            // Continue arm: TapBatchMsg received, process all events in batch
            let (ep, batch) = branch.decode::<TapBatchMsg>().await.map_err(map_recv_error)?;

            // Note: batch.lost_events() indicates ring buffer overrun count.
            // The callback can check this via TapBatch::lost_events() if needed.

            // Process each event in the batch
            for event in batch.iter() {
                let should_continue = on_event(*event);
                if !should_continue {
                    return Ok(ep);
                }
            }
            endpoint = ep;
        } else {
            // Unexpected label - protocol error
            return Err(MgmtError::InvalidTransition);
        }
    }
}
