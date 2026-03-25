use crate::{
    control::cap::mint::{
        AllowsCanonical, CAP_FIXED_HEADER_LEN, CAP_HANDLE_LEN, CAP_HEADER_LEN, CAP_NONCE_LEN,
        CAP_TAG_LEN, CAP_TOKEN_LEN, CapShot, GenericCapToken, MintConfigMarker, ResourceKind,
    },
    control::cap::resource_kinds::{
        LoadBeginKind, LoadCommitKind, LoopBreakKind, LoopContinueKind, MgmtRouteActivateKind,
        MgmtRouteCommandFamilyKind, MgmtRouteCommandTailKind, MgmtRouteLoadAndActivateKind,
        MgmtRouteLoadFamilyKind, MgmtRouteLoadKind, MgmtRouteReplyActivatedKind,
        MgmtRouteReplyErrorKind, MgmtRouteReplyLoadedKind, MgmtRouteReplyRevertedKind,
        MgmtRouteReplyStatsKind, MgmtRouteReplySuccessFamilyKind, MgmtRouteReplySuccessFinalKind,
        MgmtRouteReplySuccessTailKind, MgmtRouteRevertKind, MgmtRouteStatsKind,
    },
    endpoint::{RecvError, SendError, cursor::CursorEndpoint},
    g::{self, Program},
    global::{
        CanonicalControl, ExternalControl,
        advanced::{RoleProgram, project},
        steps::{
            self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, SeqSteps, StepConcat,
            StepCons, StepNil,
        },
    },
    observe::core::{TapBatch, TapEvent, WaitForNewUserEvents},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_MGMT_ACTIVATE, LABEL_MGMT_LOAD_AND_ACTIVATE,
        LABEL_MGMT_LOAD_BEGIN, LABEL_MGMT_LOAD_CHUNK, LABEL_MGMT_LOAD_COMMIT,
        LABEL_MGMT_LOAD_FINAL_CHUNK, LABEL_MGMT_REPLY_ACTIVATED, LABEL_MGMT_REPLY_ERROR,
        LABEL_MGMT_REPLY_LOADED, LABEL_MGMT_REPLY_REVERTED, LABEL_MGMT_REPLY_STATS,
        LABEL_MGMT_REVERT, LABEL_MGMT_ROUTE_ACTIVATE, LABEL_MGMT_ROUTE_COMMAND_FAMILY,
        LABEL_MGMT_ROUTE_COMMAND_TAIL, LABEL_MGMT_ROUTE_LOAD, LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE,
        LABEL_MGMT_ROUTE_LOAD_FAMILY, LABEL_MGMT_ROUTE_REPLY_ACTIVATED,
        LABEL_MGMT_ROUTE_REPLY_ERROR, LABEL_MGMT_ROUTE_REPLY_LOADED,
        LABEL_MGMT_ROUTE_REPLY_REVERTED, LABEL_MGMT_ROUTE_REPLY_STATS,
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL,
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, LABEL_MGMT_ROUTE_REVERT, LABEL_MGMT_ROUTE_STATS,
        LABEL_MGMT_STAGE, LABEL_MGMT_STATS, LABEL_OBSERVE_BATCH, LABEL_OBSERVE_STREAM_END,
        LABEL_OBSERVE_SUBSCRIBE,
    },
};

use super::{
    LoadBegin, LoadChunk, LoadMode, LoadReport, MgmtError, Reply, Request, RequestAction,
    SlotRequest, StatsReply, SubscribeReq,
};

pub(crate) const ROLE_CONTROLLER: u8 = 0;
pub(crate) const ROLE_CLUSTER: u8 = 1;

const LOOP_POLICY_ID: u16 = 700;
const REQUEST_ROOT_POLICY_ID: u16 = 701;
const REQUEST_LOAD_POLICY_ID: u16 = 702;
const REQUEST_COMMAND_POLICY_ID: u16 = 703;
const REQUEST_COMMAND_TAIL_POLICY_ID: u16 = 704;
const REPLY_ROOT_POLICY_ID: u16 = 705;
const REPLY_SUCCESS_POLICY_ID: u16 = 706;
const REPLY_SUCCESS_TAIL_POLICY_ID: u16 = 707;
const REPLY_SUCCESS_FINAL_POLICY_ID: u16 = 708;

type RouteMsg<const LABEL: u8, Kind> = g::Msg<LABEL, GenericCapToken<Kind>, CanonicalControl<Kind>>;
type ControllerHead<const LABEL: u8, Kind> = StepCons<
    steps::SendStep<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CONTROLLER>, RouteMsg<LABEL, Kind>>,
    StepNil,
>;
type ClusterHead<const LABEL: u8, Kind> = StepCons<
    steps::SendStep<g::Role<ROLE_CLUSTER>, g::Role<ROLE_CLUSTER>, RouteMsg<LABEL, Kind>>,
    StepNil,
>;
type ControllerSend<const LABEL: u8, Payload> = StepCons<
    steps::SendStep<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CLUSTER>, g::Msg<LABEL, Payload>>,
    StepNil,
>;
type ControllerControlSend<const LABEL: u8, Payload, Control> = StepCons<
    steps::SendStep<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL, Payload, Control>,
    >,
    StepNil,
>;
type ClusterSend<const LABEL: u8, Payload> = StepCons<
    steps::SendStep<g::Role<ROLE_CLUSTER>, g::Role<ROLE_CONTROLLER>, g::Msg<LABEL, Payload>>,
    StepNil,
>;

type LoadBeginTokenBody = ControllerControlSend<
    LABEL_MGMT_LOAD_BEGIN,
    GenericCapToken<LoadBeginKind>,
    ExternalControl<LoadBeginKind>,
>;

const LOAD_REQUEST_HEAD: Program<ControllerHead<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>,
        0,
    >()
    .policy::<REQUEST_LOAD_POLICY_ID>();

const LOAD_BEGIN: Program<LoadBeginTokenBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_LOAD_BEGIN, GenericCapToken<LoadBeginKind>, ExternalControl<LoadBeginKind>>,
    0,
>();

type LoopContinueBody = ControllerSend<LABEL_MGMT_LOAD_CHUNK, LoadChunk>;
type LoadFinalChunkBody = ControllerSend<LABEL_MGMT_LOAD_FINAL_CHUNK, LoadChunk>;
type LoopContinueArm = LoopContinueSteps<
    g::Role<ROLE_CONTROLLER>,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
    LoopContinueBody,
>;
type LoopBreakArm = LoopBreakSteps<
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    LoadFinalChunkBody,
>;

const LOOP_CONTINUE_ARM: Program<LoopContinueArm> = g::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        g::Msg<
            LABEL_LOOP_CONTINUE,
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        0,
    >()
    .policy::<LOOP_POLICY_ID>(),
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_LOAD_CHUNK, LoadChunk>,
        0,
    >(),
);

const LOOP_BREAK_PREFIX: Program<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CONTROLLER>,
            g::Role<ROLE_CONTROLLER>,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<LOOP_POLICY_ID>();

const LOAD_FINAL_CHUNK_BODY: Program<LoadFinalChunkBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_LOAD_FINAL_CHUNK, LoadChunk>,
    0,
>();

const LOOP_BREAK_ARM: Program<LoopBreakArm> =
    Program::then(LOOP_BREAK_PREFIX, LOAD_FINAL_CHUNK_BODY);

const LOOP_SEGMENT: Program<
    LoopDecisionSteps<
        g::Role<ROLE_CONTROLLER>,
        g::Msg<
            LABEL_LOOP_CONTINUE,
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        LoadFinalChunkBody,
        LoopContinueBody,
    >,
> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

type LoadCommitBody = ControllerControlSend<
    LABEL_MGMT_LOAD_COMMIT,
    GenericCapToken<LoadCommitKind>,
    ExternalControl<LoadCommitKind>,
>;

const LOAD_COMMIT_BODY: Program<LoadCommitBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<
        LABEL_MGMT_LOAD_COMMIT,
        GenericCapToken<LoadCommitKind>,
        ExternalControl<LoadCommitKind>,
    >,
    0,
>();

type LoadStreamBody = SeqSteps<
    SeqSteps<
        LoadBeginTokenBody,
        LoopDecisionSteps<
            g::Role<ROLE_CONTROLLER>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            LoadFinalChunkBody,
            LoopContinueBody,
        >,
    >,
    LoadCommitBody,
>;
type LoadRequestBody = SeqSteps<ControllerSend<LABEL_MGMT_STAGE, LoadBegin>, LoadStreamBody>;
type LoadRequestArm =
    SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>, LoadRequestBody>;
type LoadRouteSteps = <LoadRequestArm as StepConcat<LoadActivateArm>>::Output;
type LoadFamilyArm =
    SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>, LoadRouteSteps>;
type LoadActivateBody =
    SeqSteps<ControllerSend<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>, LoadStreamBody>;
type LoadActivateArm = SeqSteps<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
    LoadActivateBody,
>;

const LOAD_ACTIVATE_HEAD: Program<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
    0,
>()
.policy::<REQUEST_LOAD_POLICY_ID>();

const LOAD_STREAM_BODY: Program<LoadStreamBody> = crate::g::advanced::compose::seq(
    crate::g::advanced::compose::seq(LOAD_BEGIN, LOOP_SEGMENT),
    LOAD_COMMIT_BODY,
);

const LOAD_REQUEST_BODY: Program<LoadRequestBody> = crate::g::advanced::compose::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_STAGE, LoadBegin>,
        0,
    >(),
    LOAD_STREAM_BODY,
);

const LOAD_REQUEST: Program<LoadRequestArm> = g::seq(LOAD_REQUEST_HEAD, LOAD_REQUEST_BODY);

const LOAD_ACTIVATE_BODY: Program<LoadActivateBody> = crate::g::advanced::compose::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>,
        0,
    >(),
    LOAD_STREAM_BODY,
);

const LOAD_ACTIVATE_REQUEST: Program<LoadActivateArm> =
    g::seq(LOAD_ACTIVATE_HEAD, LOAD_ACTIVATE_BODY);

const LOAD_FAMILY_HEAD: Program<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>,
    0,
>()
.policy::<REQUEST_ROOT_POLICY_ID>();

const LOAD_ROUTE: Program<LoadRouteSteps> = g::route(LOAD_REQUEST, LOAD_ACTIVATE_REQUEST);
const LOAD_FAMILY_REQUEST: Program<LoadFamilyArm> = g::seq(LOAD_FAMILY_HEAD, LOAD_ROUTE);

type ActivateBody = ControllerSend<LABEL_MGMT_ACTIVATE, SlotRequest>;
type ActivateArm =
    SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>, ActivateBody>;

const ACTIVATE_HEAD: Program<ControllerHead<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>,
        0,
    >()
    .policy::<REQUEST_COMMAND_POLICY_ID>();

const ACTIVATE_BODY: Program<ActivateBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_ACTIVATE, SlotRequest>,
    0,
>();

const ACTIVATE_REQUEST: Program<ActivateArm> = g::seq(ACTIVATE_HEAD, ACTIVATE_BODY);

type RevertBody = ControllerSend<LABEL_MGMT_REVERT, SlotRequest>;
type RevertArm = SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>, RevertBody>;

const REVERT_HEAD: Program<ControllerHead<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>,
        0,
    >()
    .policy::<REQUEST_COMMAND_TAIL_POLICY_ID>();

const REVERT_BODY: Program<RevertBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_REVERT, SlotRequest>,
    0,
>();

const REVERT_REQUEST: Program<RevertArm> = g::seq(REVERT_HEAD, REVERT_BODY);

type StatsBody = ControllerSend<LABEL_MGMT_STATS, SlotRequest>;
type StatsArm = SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>, StatsBody>;

const STATS_HEAD: Program<ControllerHead<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>,
    0,
>()
.policy::<REQUEST_COMMAND_TAIL_POLICY_ID>();

const STATS_BODY: Program<StatsBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_STATS, SlotRequest>,
    0,
>();

const STATS_REQUEST: Program<StatsArm> = g::seq(STATS_HEAD, STATS_BODY);

type CommandTailRouteSteps = <RevertArm as StepConcat<StatsArm>>::Output;
type CommandTailArm = SeqSteps<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>,
    CommandTailRouteSteps,
>;
type CommandRouteSteps = <ActivateArm as StepConcat<CommandTailArm>>::Output;
type CommandFamilyArm = SeqSteps<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>,
    CommandRouteSteps,
>;
type RequestRouteSteps = <LoadFamilyArm as StepConcat<CommandFamilyArm>>::Output;

const COMMAND_TAIL_HEAD: Program<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>,
    0,
>()
.policy::<REQUEST_COMMAND_POLICY_ID>();

const COMMAND_TAIL_ROUTE: Program<CommandTailRouteSteps> = g::route(REVERT_REQUEST, STATS_REQUEST);
const COMMAND_TAIL_REQUEST: Program<CommandTailArm> = g::seq(COMMAND_TAIL_HEAD, COMMAND_TAIL_ROUTE);

const COMMAND_ROUTE: Program<CommandRouteSteps> = g::route(ACTIVATE_REQUEST, COMMAND_TAIL_REQUEST);

const COMMAND_FAMILY_HEAD: Program<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>,
    0,
>()
.policy::<REQUEST_ROOT_POLICY_ID>();

const COMMAND_FAMILY_REQUEST: Program<CommandFamilyArm> =
    g::seq(COMMAND_FAMILY_HEAD, COMMAND_ROUTE);
const REQUEST_ROUTE: Program<RequestRouteSteps> =
    g::route(LOAD_FAMILY_REQUEST, COMMAND_FAMILY_REQUEST);

type ErrorReplyBody = ClusterSend<LABEL_MGMT_REPLY_ERROR, MgmtError>;
type ErrorReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>, ErrorReplyBody>;

const ERROR_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>,
    0,
>()
.policy::<REPLY_ROOT_POLICY_ID>();

const ERROR_REPLY_BODY: Program<ErrorReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_ERROR, MgmtError>,
    0,
>();

const ERROR_REPLY: Program<ErrorReplyArm> = g::seq(ERROR_REPLY_HEAD, ERROR_REPLY_BODY);

type LoadedReplyBody = ClusterSend<LABEL_MGMT_REPLY_LOADED, LoadReport>;
type LoadedReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>, LoadedReplyBody>;

const LOADED_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_POLICY_ID>();

const LOADED_REPLY_BODY: Program<LoadedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_LOADED, LoadReport>,
    0,
>();

const LOADED_REPLY: Program<LoadedReplyArm> = g::seq(LOADED_REPLY_HEAD, LOADED_REPLY_BODY);

type ActivatedReplyBody = ClusterSend<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>;
type ActivatedReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
    ActivatedReplyBody,
>;

const ACTIVATED_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_TAIL_POLICY_ID>();

const ACTIVATED_REPLY_BODY: Program<ActivatedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>,
    0,
>();

const ACTIVATED_REPLY: Program<ActivatedReplyArm> =
    g::seq(ACTIVATED_REPLY_HEAD, ACTIVATED_REPLY_BODY);

type RevertedReplyBody = ClusterSend<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>;
type RevertedReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
    RevertedReplyBody,
>;

const REVERTED_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_FINAL_POLICY_ID>();

const REVERTED_REPLY_BODY: Program<RevertedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>,
    0,
>();

const REVERTED_REPLY: Program<RevertedReplyArm> = g::seq(REVERTED_REPLY_HEAD, REVERTED_REPLY_BODY);

type StatsReplyBody = ClusterSend<LABEL_MGMT_REPLY_STATS, StatsReply>;
type StatsReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>, StatsReplyBody>;

const STATS_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>,
    0,
>()
.policy::<REPLY_SUCCESS_FINAL_POLICY_ID>();

const STATS_REPLY_BODY: Program<StatsReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_STATS, StatsReply>,
    0,
>();

const STATS_REPLY: Program<StatsReplyArm> = g::seq(STATS_REPLY_HEAD, STATS_REPLY_BODY);

type SuccessFinalReplyRouteSteps = <RevertedReplyArm as StepConcat<StatsReplyArm>>::Output;
type SuccessFinalReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
    SuccessFinalReplyRouteSteps,
>;
type SuccessTailReplyRouteSteps = <ActivatedReplyArm as StepConcat<SuccessFinalReplyArm>>::Output;
type SuccessTailReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
    SuccessTailReplyRouteSteps,
>;
type SuccessReplyRouteSteps = <LoadedReplyArm as StepConcat<SuccessTailReplyArm>>::Output;
type SuccessReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>,
    SuccessReplyRouteSteps,
>;
type ReplyRouteSteps = <ErrorReplyArm as StepConcat<SuccessReplyArm>>::Output;
type ProgramSteps = SeqSteps<RequestRouteSteps, ReplyRouteSteps>;

const SUCCESS_FINAL_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
    0,
>()
.policy::<REPLY_SUCCESS_TAIL_POLICY_ID>();

const SUCCESS_TAIL_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
    0,
>()
.policy::<REPLY_SUCCESS_POLICY_ID>();

const SUCCESS_REPLY_HEAD: Program<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>,
    0,
>()
.policy::<REPLY_ROOT_POLICY_ID>();

const SUCCESS_FINAL_REPLY_ROUTE: Program<SuccessFinalReplyRouteSteps> =
    g::route(REVERTED_REPLY, STATS_REPLY);
const SUCCESS_FINAL_REPLY_FAMILY: Program<SuccessFinalReplyArm> =
    g::seq(SUCCESS_FINAL_REPLY_HEAD, SUCCESS_FINAL_REPLY_ROUTE);
const SUCCESS_TAIL_REPLY_ROUTE: Program<SuccessTailReplyRouteSteps> =
    g::route(ACTIVATED_REPLY, SUCCESS_FINAL_REPLY_FAMILY);
const SUCCESS_TAIL_REPLY_FAMILY: Program<SuccessTailReplyArm> =
    g::seq(SUCCESS_TAIL_REPLY_HEAD, SUCCESS_TAIL_REPLY_ROUTE);
const SUCCESS_REPLY_ROUTE: Program<SuccessReplyRouteSteps> =
    g::route(LOADED_REPLY, SUCCESS_TAIL_REPLY_FAMILY);
const SUCCESS_REPLY_FAMILY: Program<SuccessReplyArm> =
    g::seq(SUCCESS_REPLY_HEAD, SUCCESS_REPLY_ROUTE);
const REPLY_ROUTE: Program<ReplyRouteSteps> = g::route(ERROR_REPLY, SUCCESS_REPLY_FAMILY);
const PROGRAM: Program<ProgramSteps> = crate::g::advanced::compose::seq(REQUEST_ROUTE, REPLY_ROUTE);

static CONTROLLER_PROGRAM: RoleProgram<
    'static,
    ROLE_CONTROLLER,
    <ProgramSteps as steps::ProjectRole<g::Role<ROLE_CONTROLLER>>>::Output,
> = project(&PROGRAM);

static CLUSTER_PROGRAM: RoleProgram<
    'static,
    ROLE_CLUSTER,
    <ProgramSteps as steps::ProjectRole<g::Role<ROLE_CLUSTER>>>::Output,
> = project(&PROGRAM);

const _: () = crate::control::lease::planner::assert_program_covers_facets(
    &CONTROLLER_PROGRAM,
    super::MGMT_FACET_NEEDS,
);

const _: () = crate::control::lease::planner::assert_program_covers_facets(
    &CLUSTER_PROGRAM,
    super::MGMT_FACET_NEEDS,
);

#[cfg(test)]
pub(super) fn management_eff_lists() -> (
    &'static crate::global::const_dsl::EffList,
    &'static crate::global::const_dsl::EffList,
) {
    (CONTROLLER_PROGRAM.eff_list(), CLUSTER_PROGRAM.eff_list())
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
    cluster.enter(rv_id, sid, &CONTROLLER_PROGRAM, binding)
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
    cluster.enter(rv_id, sid, &CLUSTER_PROGRAM, binding)
}

fn make_external_token<K: ResourceKind>(handle: &K::Handle) -> GenericCapToken<K> {
    let handle_bytes = K::encode_handle(handle);
    let mask = K::caps_mask(handle);

    let mut header = [0u8; CAP_HEADER_LEN];
    header[0..4].copy_from_slice(&0u32.to_be_bytes());
    header[4] = 0;
    header[5] = 0;
    header[6] = K::TAG;
    header[7] = CapShot::One.as_u8();
    header[8..10].copy_from_slice(&mask.bits().to_be_bytes());
    header[CAP_FIXED_HEADER_LEN..CAP_FIXED_HEADER_LEN + CAP_HANDLE_LEN]
        .copy_from_slice(&handle_bytes);

    let mut bytes = [0u8; CAP_TOKEN_LEN];
    bytes[..CAP_NONCE_LEN].fill(0);
    bytes[CAP_NONCE_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN].copy_from_slice(&header);
    bytes[CAP_NONCE_LEN + CAP_HEADER_LEN..CAP_NONCE_LEN + CAP_HEADER_LEN + CAP_TAG_LEN].fill(0);
    GenericCapToken::from_bytes(bytes)
}

fn slot_handle_id(slot: crate::epf::vm::Slot) -> u8 {
    match slot {
        crate::epf::vm::Slot::Forward => 0,
        crate::epf::vm::Slot::EndpointRx => 1,
        crate::epf::vm::Slot::EndpointTx => 2,
        crate::epf::vm::Slot::Rendezvous => 3,
        crate::epf::vm::Slot::Route => 4,
    }
}

async fn send_load_family_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn send_command_family_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn send_command_tail_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn send_reply_success_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn send_reply_success_tail_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn send_reply_success_final_head<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (endpoint, _) = endpoint
        .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>>()
        .map_err(map_send_error)?
        .send(())
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

pub(crate) async fn drive_controller<'lease, 'request, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
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
        CursorEndpoint<
            'lease,
            ROLE_CONTROLLER,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        Reply,
    ),
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let endpoint = match request {
        Request::Load(load) => drive_load_request(endpoint, load, false).await?,
        Request::LoadAndActivate(load) => drive_load_request(endpoint, load, true).await?,
        Request::Activate(slot) => {
            let endpoint = send_command_family_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_ACTIVATE, SlotRequest>>()
                .map_err(map_send_error)?
                .send(&slot)
                .await
                .map_err(map_send_error)?;
            endpoint
        }
        Request::Revert(slot) => {
            let endpoint = send_command_family_head(endpoint).await?;
            let endpoint = send_command_tail_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REVERT, SlotRequest>>()
                .map_err(map_send_error)?
                .send(&slot)
                .await
                .map_err(map_send_error)?;
            endpoint
        }
        Request::Stats(slot) => {
            let endpoint = send_command_family_head(endpoint).await?;
            let endpoint = send_command_tail_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_STATS, SlotRequest>>()
                .map_err(map_send_error)?
                .send(&slot)
                .await
                .map_err(map_send_error)?;
            endpoint
        }
    };

    recv_reply(endpoint).await
}

pub(crate) async fn drive_cluster<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(
    cluster: &'lease crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let branch = endpoint.offer().await.map_err(map_recv_error)?;
    match branch.label() {
        LABEL_MGMT_STAGE => {
            let (endpoint, begin) = branch
                .decode::<g::Msg<LABEL_MGMT_STAGE, LoadBegin>>()
                .await
                .map_err(map_recv_error)?;
            drive_load_branch(cluster, rv_id, sid, endpoint, begin, LoadMode::Stage).await
        }
        LABEL_MGMT_LOAD_AND_ACTIVATE => {
            let (endpoint, begin) = match branch
                .decode::<g::Msg<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>>()
                .await
            {
                Ok(value) => value,
                Err(err) => return Err(map_recv_error(err)),
            };
            drive_load_branch(cluster, rv_id, sid, endpoint, begin, LoadMode::Activate).await
        }
        LABEL_MGMT_ACTIVATE => {
            let (endpoint, slot) = branch
                .decode::<g::Msg<LABEL_MGMT_ACTIVATE, SlotRequest>>()
                .await
                .map_err(map_recv_error)?;
            let manager = cluster.take_mgmt_manager(rv_id);
            let backup = manager.clone();
            let result = super::apply_request_action(
                cluster,
                rv_id,
                sid,
                backup,
                manager,
                RequestAction::Activate { slot: slot.slot },
            );
            send_reply(endpoint, result).await
        }
        LABEL_MGMT_REVERT => {
            let (endpoint, slot) = branch
                .decode::<g::Msg<LABEL_MGMT_REVERT, SlotRequest>>()
                .await
                .map_err(map_recv_error)?;
            let manager = cluster.take_mgmt_manager(rv_id);
            let backup = manager.clone();
            let result = super::apply_request_action(
                cluster,
                rv_id,
                sid,
                backup,
                manager,
                RequestAction::Revert { slot: slot.slot },
            );
            send_reply(endpoint, result).await
        }
        LABEL_MGMT_STATS => {
            let (endpoint, slot) = branch
                .decode::<g::Msg<LABEL_MGMT_STATS, SlotRequest>>()
                .await
                .map_err(map_recv_error)?;
            let manager = cluster.take_mgmt_manager(rv_id);
            let backup = manager.clone();
            let result = super::apply_request_action(
                cluster,
                rv_id,
                sid,
                backup,
                manager,
                RequestAction::Stats { slot: slot.slot },
            );
            send_reply(endpoint, result).await
        }
        _ => Err(MgmtError::InvalidTransition),
    }
}

async fn drive_load_branch<'lease, 'cfg, T, U, C, Mint, B, const MAX_RV: usize>(
    cluster: &'lease crate::control::cluster::core::SessionCluster<'cfg, T, U, C, MAX_RV>,
    rv_id: crate::control::types::RendezvousId,
    sid: crate::control::types::SessionId,
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    begin: LoadBegin,
    mode: LoadMode,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'cfg,
    U: crate::runtime::consts::LabelUniverse + 'cfg,
    C: crate::runtime::config::Clock + 'cfg,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let (mut endpoint, _) = endpoint
        .recv::<g::Msg<
            LABEL_MGMT_LOAD_BEGIN,
            GenericCapToken<LoadBeginKind>,
            ExternalControl<LoadBeginKind>,
        >>()
        .await
        .map_err(map_recv_error)?;

    let mut manager = cluster.take_mgmt_manager(rv_id);
    let backup = manager.clone();
    if let Err(err) = manager.load_begin(
        begin.slot,
        crate::epf::verifier::Header {
            code_len: begin.code_len as u16,
            fuel_max: begin.fuel_max,
            mem_len: begin.mem_len,
            flags: 0,
            hash: begin.hash,
        },
    ) {
        cluster.store_mgmt_manager(rv_id, backup);
        return send_reply(endpoint, Err(err)).await;
    }

    loop {
        let branch = endpoint.offer().await.map_err(map_recv_error)?;
        match branch.label() {
            LABEL_MGMT_LOAD_CHUNK => {
                let (next, chunk) = branch
                    .decode::<g::Msg<LABEL_MGMT_LOAD_CHUNK, LoadChunk>>()
                    .await
                    .map_err(map_recv_error)?;
                if let Err(err) = manager.load_chunk(begin.slot, chunk.offset, chunk.bytes()) {
                    cluster.store_mgmt_manager(rv_id, backup);
                    return send_reply(next, Err(err)).await;
                }
                endpoint = next;
            }
            LABEL_MGMT_LOAD_FINAL_CHUNK => {
                let (next, chunk) = branch
                    .decode::<g::Msg<LABEL_MGMT_LOAD_FINAL_CHUNK, LoadChunk>>()
                    .await
                    .map_err(map_recv_error)?;
                if let Err(err) = manager.load_chunk(begin.slot, chunk.offset, chunk.bytes()) {
                    cluster.store_mgmt_manager(rv_id, backup);
                    return send_reply(next, Err(err)).await;
                }
                endpoint = next;
                break;
            }
            _ => {
                cluster.store_mgmt_manager(rv_id, backup);
                return send_reply(branch.into_endpoint(), Err(MgmtError::InvalidTransition)).await;
            }
        }
    }

    let (endpoint, _) = endpoint
        .recv::<g::Msg<
            LABEL_MGMT_LOAD_COMMIT,
            GenericCapToken<LoadCommitKind>,
            ExternalControl<LoadCommitKind>,
        >>()
        .await
        .map_err(map_recv_error)?;
    let result = super::apply_request_action(
        cluster,
        rv_id,
        sid,
        backup,
        manager,
        RequestAction::Load {
            slot: begin.slot,
            mode,
        },
    );
    send_reply(endpoint, result).await
}

async fn drive_load_request<'lease, 'request, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    load: super::LoadRequest<'request>,
    activate: bool,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let header = LoadBegin {
        slot: load.slot,
        code_len: u32::try_from(load.code.len()).map_err(|_| MgmtError::ChunkTooLarge {
            remaining: 0,
            provided: load.code.len() as u32,
        })?,
        fuel_max: load.fuel_max,
        mem_len: load.mem_len,
        hash: crate::epf::verifier::compute_hash(load.code),
    };
    let load_begin_token =
        make_external_token::<LoadBeginKind>(&(slot_handle_id(load.slot), u64::from(header.hash)));
    let endpoint = send_load_family_head(endpoint).await?;
    let endpoint = if activate {
        let (endpoint, _) = endpoint
            .flow::<RouteMsg<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>>()
            .map_err(map_send_error)?
            .send(())
            .await
            .map_err(map_send_error)?;
        let (endpoint, _) = endpoint
            .flow::<g::Msg<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>>()
            .map_err(map_send_error)?
            .send(&header)
            .await
            .map_err(map_send_error)?;
        endpoint
    } else {
        let (endpoint, _) = endpoint
            .flow::<RouteMsg<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>>()
            .map_err(map_send_error)?
            .send(())
            .await
            .map_err(map_send_error)?;
        let (endpoint, _) = endpoint
            .flow::<g::Msg<LABEL_MGMT_STAGE, LoadBegin>>()
            .map_err(map_send_error)?
            .send(&header)
            .await
            .map_err(map_send_error)?;
        endpoint
    };
    let (mut endpoint, _) = endpoint
        .flow::<g::Msg<
            LABEL_MGMT_LOAD_BEGIN,
            GenericCapToken<LoadBeginKind>,
            ExternalControl<LoadBeginKind>,
        >>()
        .map_err(map_send_error)?
        .send(&load_begin_token)
        .await
        .map_err(map_send_error)?;

    let final_start = if load.code.is_empty() {
        0
    } else {
        ((load.code.len() - 1) / 1024) * 1024
    };
    let mut offset = 0usize;
    while offset < final_start {
        let end = core::cmp::min(offset + 1024, final_start);
        let chunk = LoadChunk::new(offset as u32, &load.code[offset..end]);
        let (next, _) = endpoint
            .flow::<g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >>()
            .map_err(map_send_error)?
            .send(())
            .await
            .map_err(map_send_error)?;
        let (next, _) = next
            .flow::<g::Msg<LABEL_MGMT_LOAD_CHUNK, LoadChunk>>()
            .map_err(map_send_error)?
            .send(&chunk)
            .await
            .map_err(map_send_error)?;
        endpoint = next;
        offset = end;
    }

    let final_chunk = LoadChunk::new(offset as u32, &load.code[offset..]);
    let (endpoint, _) =
        endpoint
            .flow::<g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >>()
            .map_err(map_send_error)?
            .send(())
            .await
            .map_err(map_send_error)?;
    let (endpoint, _) = endpoint
        .flow::<g::Msg<LABEL_MGMT_LOAD_FINAL_CHUNK, LoadChunk>>()
        .map_err(map_send_error)?
        .send(&final_chunk)
        .await
        .map_err(map_send_error)?;

    let load_commit_token = make_external_token::<LoadCommitKind>(&slot_handle_id(load.slot));
    let (endpoint, _) = endpoint
        .flow::<g::Msg<
            LABEL_MGMT_LOAD_COMMIT,
            GenericCapToken<LoadCommitKind>,
            ExternalControl<LoadCommitKind>,
        >>()
        .map_err(map_send_error)?
        .send(&load_commit_token)
        .await
        .map_err(map_send_error)?;
    Ok(endpoint)
}

async fn recv_reply<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
) -> Result<
    (
        CursorEndpoint<
            'lease,
            ROLE_CONTROLLER,
            T,
            U,
            C,
            crate::control::cap::mint::EpochTbl,
            MAX_RV,
            Mint,
            B,
        >,
        Reply,
    ),
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    let branch = endpoint.offer().await.map_err(map_recv_error)?;
    match branch.label() {
        LABEL_MGMT_REPLY_ERROR => {
            let (_, err) = branch
                .decode::<g::Msg<LABEL_MGMT_REPLY_ERROR, MgmtError>>()
                .await
                .map_err(map_recv_error)?;
            Err(err)
        }
        LABEL_MGMT_REPLY_LOADED => {
            let (endpoint, payload) = branch
                .decode::<g::Msg<LABEL_MGMT_REPLY_LOADED, LoadReport>>()
                .await
                .map_err(map_recv_error)?;
            Ok((endpoint, Reply::Loaded(payload)))
        }
        LABEL_MGMT_REPLY_ACTIVATED => {
            let (endpoint, payload) = branch
                .decode::<g::Msg<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>>()
                .await
                .map_err(map_recv_error)?;
            Ok((endpoint, Reply::ActivationScheduled(payload)))
        }
        LABEL_MGMT_REPLY_REVERTED => {
            let (endpoint, payload) = branch
                .decode::<g::Msg<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>>()
                .await
                .map_err(map_recv_error)?;
            Ok((endpoint, Reply::Reverted(payload)))
        }
        LABEL_MGMT_REPLY_STATS => {
            let (endpoint, payload) = branch
                .decode::<g::Msg<LABEL_MGMT_REPLY_STATS, StatsReply>>()
                .await
                .map_err(map_recv_error)?;
            Ok((
                endpoint,
                Reply::Stats {
                    stats: payload.stats,
                    staged_version: payload.staged_version,
                },
            ))
        }
        _ => Err(MgmtError::InvalidTransition),
    }
}

async fn send_reply<'lease, T, U, C, Mint, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    result: Result<Reply, MgmtError>,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    B: crate::binding::BindingSlot,
{
    match result {
        Ok(Reply::Loaded(report)) => {
            let endpoint = send_reply_success_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REPLY_LOADED, LoadReport>>()
                .map_err(map_send_error)?
                .send(&report)
                .await
                .map_err(map_send_error)?;
            Ok(endpoint)
        }
        Ok(Reply::ActivationScheduled(report)) => {
            let endpoint = send_reply_success_head(endpoint).await?;
            let endpoint = send_reply_success_tail_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>>()
                .map_err(map_send_error)?
                .send(&report)
                .await
                .map_err(map_send_error)?;
            Ok(endpoint)
        }
        Ok(Reply::Reverted(report)) => {
            let endpoint = send_reply_success_head(endpoint).await?;
            let endpoint = send_reply_success_tail_head(endpoint).await?;
            let endpoint = send_reply_success_final_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>>()
                .map_err(map_send_error)?
                .send(&report)
                .await
                .map_err(map_send_error)?;
            Ok(endpoint)
        }
        Ok(Reply::Stats {
            stats,
            staged_version,
        }) => {
            let payload = StatsReply {
                stats,
                staged_version,
            };
            let endpoint = send_reply_success_head(endpoint).await?;
            let endpoint = send_reply_success_tail_head(endpoint).await?;
            let endpoint = send_reply_success_final_head(endpoint).await?;
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REPLY_STATS, StatsReply>>()
                .map_err(map_send_error)?
                .send(&payload)
                .await
                .map_err(map_send_error)?;
            Ok(endpoint)
        }
        Err(err) => {
            let (endpoint, _) = endpoint
                .flow::<RouteMsg<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>>()
                .map_err(map_send_error)?
                .send(())
                .await
                .map_err(map_send_error)?;
            let (endpoint, _) = endpoint
                .flow::<g::Msg<LABEL_MGMT_REPLY_ERROR, MgmtError>>()
                .map_err(map_send_error)?
                .send(&err)
                .await
                .map_err(map_send_error)?;
            Ok(endpoint)
        }
    }
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
    cluster.enter(rv_id, sid, &STREAM_CONTROLLER_PROGRAM, binding)
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
    cluster.enter(rv_id, sid, &STREAM_CLUSTER_PROGRAM, binding)
}

fn map_recv_error(_err: RecvError) -> MgmtError {
    MgmtError::InvalidTransition
}

fn map_send_error(_err: SendError) -> MgmtError {
    MgmtError::InvalidTransition
}

// ============================================================================
// Streaming Observe Choreography
// ============================================================================
//
// Real-time event streaming: Controller subscribes, Cluster streams TapEvents.
//
// Design: Use the same pattern as the transport stream-loop choreography.
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
//   - Controller: offer() to distinguish TapBatch (arm0) vs StreamEnd (arm1)
//
// The end-of-stream message (label 39) signals stream termination instead of a batch sentinel.

// Stream loop arms:
// - Continue arm: self-send Continue → TapBatch (loops via linger)
// - Break arm: self-send Break → StreamEnd (exits loop)
// Different message types allow Controller to distinguish via offer().
const STREAM_SUBSCRIBE: Program<
    StepCons<
        steps::SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>,
        StepNil,
    >,
> = g::send::<g::Role<0>, g::Role<1>, g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>, 0>();

// Stream loop arms (Cluster self-send for CanonicalControl)
const STREAM_LOOP_CONTINUE_PREFIX: Program<
    StepCons<
        steps::SendStep<
            g::Role<1>,
            g::Role<1>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
> = g::send::<
    g::Role<1>,
    g::Role<1>,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
    0,
>()
.policy::<STREAM_LOOP_POLICY_ID>();
const STREAM_LOOP_CONTINUE_ARM: Program<
    SeqSteps<
        StepCons<
            steps::SendStep<
                g::Role<1>,
                g::Role<1>,
                g::Msg<
                    LABEL_LOOP_CONTINUE,
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
            >,
            StepNil,
        >,
        StepCons<
            steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
            StepNil,
        >,
    >,
> = g::seq(
    STREAM_LOOP_CONTINUE_PREFIX,
    g::send::<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>, 0>(),
);

// Break arm sends the end-of-stream message (distinct label from TapBatch).
const STREAM_LOOP_BREAK_PREFIX: Program<
    StepCons<
        steps::SendStep<
            g::Role<1>,
            g::Role<1>,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
> = g::send::<
    g::Role<1>,
    g::Role<1>,
    g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<STREAM_LOOP_POLICY_ID>();
const STREAM_LOOP_BREAK_ARM: Program<
    LoopBreakSteps<
        g::Role<1>,
        g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepCons<
            steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
            StepNil,
        >,
    >,
> = STREAM_LOOP_BREAK_PREFIX.then(g::send::<
    g::Role<1>,
    g::Role<0>,
    g::Msg<LABEL_OBSERVE_STREAM_END, ()>,
    0,
>());

const STREAM_LOOP_POLICY_ID: u16 = 701;

// Route is local to Cluster (1 → 1) and preserves loop semantics through the
// typed loop control messages at the arm heads.
const STREAM_LOOP_ROUTE: Program<
    LoopDecisionSteps<
        g::Role<1>,
        g::Msg<
            LABEL_LOOP_CONTINUE,
            GenericCapToken<LoopContinueKind>,
            CanonicalControl<LoopContinueKind>,
        >,
        g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
        StepCons<
            steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
            StepNil,
        >,
        StepCons<
            steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
            StepNil,
        >,
    >,
> = g::route(STREAM_LOOP_CONTINUE_ARM, STREAM_LOOP_BREAK_ARM);

const STREAM_PROGRAM: Program<
    SeqSteps<
        StepCons<
            steps::SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>,
            StepNil,
        >,
        LoopDecisionSteps<
            g::Role<1>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
                StepNil,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
                StepNil,
            >,
        >,
    >,
> = crate::g::advanced::compose::seq(STREAM_SUBSCRIBE, STREAM_LOOP_ROUTE);

static STREAM_CONTROLLER_PROGRAM: RoleProgram<
    'static,
    ROLE_CONTROLLER,
    <SeqSteps<
        StepCons<
            steps::SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>,
            StepNil,
        >,
        LoopDecisionSteps<
            g::Role<1>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
                StepNil,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
                StepNil,
            >,
        >,
    > as steps::ProjectRole<g::Role<0>>>::Output,
> = project(&STREAM_PROGRAM);
static STREAM_CLUSTER_PROGRAM: RoleProgram<
    'static,
    ROLE_CLUSTER,
    <SeqSteps<
        StepCons<
            steps::SendStep<g::Role<0>, g::Role<1>, g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>,
            StepNil,
        >,
        LoopDecisionSteps<
            g::Role<1>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
                StepNil,
            >,
            StepCons<
                steps::SendStep<g::Role<1>, g::Role<0>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
                StepNil,
            >,
        >,
    > as steps::ProjectRole<g::Role<1>>>::Output,
> = project(&STREAM_PROGRAM);

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
pub(crate) async fn drive_stream_cluster<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    mut should_continue: F,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CLUSTER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    MgmtError,
>
where
    T: crate::transport::Transport + 'lease,
    U: crate::runtime::consts::LabelUniverse,
    C: crate::runtime::config::Clock,
    Mint: MintConfigMarker,
    Mint::Policy: AllowsCanonical,
    F: FnMut() -> bool,
    B: crate::binding::BindingSlot,
{
    // Receive subscribe request
    let (mut endpoint, _subscribe) = endpoint
        .recv::<g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>()
        .await
        .map_err(map_recv_error)?;

    let mut cursor: usize = crate::observe::core::user_head().unwrap_or(0);

    // Stream loop: Continue → TapBatch | Break
    // Cluster is the loop controller (sender of self-send Continue/Break)
    // Greedy Batch: fill batch from ring buffer in single iteration
    loop {
        // Wait for at least one new event
        let new_head = WaitForNewUserEvents::new(cursor).await;

        // Check if we should continue streaming
        if !should_continue() {
            // Break arm: self-send Break, then send the end-of-stream message
            let (ep, outcome) = endpoint
                .flow::<g::Msg<
                    LABEL_LOOP_BREAK,
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >>()
                .map_err(|err| map_send_error(err.into()))?
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(outcome.is_canonical());

            // Send the end-of-stream message to signal termination
            let (ep, _) = ep
                .flow::<g::Msg<LABEL_OBSERVE_STREAM_END, ()>>()
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
            if let Some(event) = crate::observe::core::read_user_at(cursor) {
                batch.push(event);
            }
            cursor += 1;
        }

        if batch.len() > 0 {
            // Continue arm: self-send Continue, then send TapBatch
            let (ep, outcome) = endpoint
                .flow::<g::Msg<
                    LABEL_LOOP_CONTINUE,
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >>()
                .map_err(|err| map_send_error(err.into()))?
                .send(())
                .await
                .map_err(map_send_error)?;
            debug_assert!(outcome.is_canonical());

            // Send TapBatch to Controller
            let (ep, _) = ep
                .flow::<g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>()
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
pub(crate) async fn drive_stream_controller<'lease, T, U, C, Mint, F, B, const MAX_RV: usize>(
    endpoint: CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
    subscribe: SubscribeReq,
    mut on_event: F,
) -> Result<
    CursorEndpoint<
        'lease,
        ROLE_CONTROLLER,
        T,
        U,
        C,
        crate::control::cap::mint::EpochTbl,
        MAX_RV,
        Mint,
        B,
    >,
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
        .flow::<g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>>()
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
            // Break arm: end-of-stream message received, exit loop
            let (ep, ()) = branch
                .decode::<g::Msg<LABEL_OBSERVE_STREAM_END, ()>>()
                .await
                .map_err(map_recv_error)?;
            return Ok(ep);
        } else if label == LABEL_OBSERVE_BATCH {
            // Continue arm: TapBatch received, process all events in batch
            let (ep, batch) = branch
                .decode::<g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>()
                .await
                .map_err(map_recv_error)?;

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
