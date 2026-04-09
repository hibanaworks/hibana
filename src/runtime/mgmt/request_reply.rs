use crate::{
    control::{
        cap::{
            mint::GenericCapToken,
            resource_kinds::{
                LoadBeginKind, LoadCommitKind, LoopBreakKind, LoopContinueKind,
                MgmtRouteActivateKind, MgmtRouteCommandFamilyKind, MgmtRouteCommandTailKind,
                MgmtRouteLoadAndActivateKind, MgmtRouteLoadFamilyKind, MgmtRouteLoadKind,
                MgmtRouteReplyActivatedKind, MgmtRouteReplyErrorKind, MgmtRouteReplyLoadedKind,
                MgmtRouteReplyRevertedKind, MgmtRouteReplyStatsKind,
                MgmtRouteReplySuccessFamilyKind, MgmtRouteReplySuccessFinalKind,
                MgmtRouteReplySuccessTailKind, MgmtRouteRevertKind, MgmtRouteStatsKind,
            },
        },
        lease::planner::{LeaseFacetNeeds, assert_program_covers_facets, facets_slots},
    },
    g::{self, ProgramSource},
    global::{
        CanonicalControl, ExternalControl,
        steps::{
            self, LoopBreakSteps, LoopContinueSteps, LoopDecisionSteps, SeqSteps, StepConcat,
            StepCons, StepNil,
        },
    },
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
        LABEL_MGMT_STAGE, LABEL_MGMT_STATS,
    },
};

#[cfg(test)]
use crate::global::advanced::{RoleProgram, project};

use super::payload::{LoadBegin, LoadChunk, LoadReport, MgmtError, SlotRequest, StatsReply};

pub const ROLE_CONTROLLER: u8 = 0;
pub const ROLE_CLUSTER: u8 = 1;

const MGMT_FACET_NEEDS: LeaseFacetNeeds = facets_slots();
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

const LOAD_REQUEST_HEAD: ProgramSource<ControllerHead<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind>,
        0,
    >()
    .policy::<REQUEST_LOAD_POLICY_ID>();

const LOAD_BEGIN: ProgramSource<LoadBeginTokenBody> = g::send::<
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

const LOOP_CONTINUE_ARM: ProgramSource<LoopContinueArm> = g::seq(
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

const LOOP_BREAK_PREFIX: ProgramSource<
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

const LOAD_FINAL_CHUNK_BODY: ProgramSource<LoadFinalChunkBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_LOAD_FINAL_CHUNK, LoadChunk>,
    0,
>();

const LOOP_BREAK_ARM: ProgramSource<LoopBreakArm> =
    ProgramSource::then(LOOP_BREAK_PREFIX, LOAD_FINAL_CHUNK_BODY);

const LOOP_SEGMENT: ProgramSource<
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

const LOAD_COMMIT_BODY: ProgramSource<LoadCommitBody> = g::send::<
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
type LoadActivateBody =
    SeqSteps<ControllerSend<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>, LoadStreamBody>;
type LoadActivateArm = SeqSteps<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
    LoadActivateBody,
>;
type LoadRouteSteps = <LoadRequestArm as StepConcat<LoadActivateArm>>::Output;
type LoadFamilyArm =
    SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>, LoadRouteSteps>;

const LOAD_ACTIVATE_HEAD: ProgramSource<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
    0,
>()
.policy::<REQUEST_LOAD_POLICY_ID>();

const LOAD_STREAM_BODY: ProgramSource<LoadStreamBody> =
    crate::g::seq(crate::g::seq(LOAD_BEGIN, LOOP_SEGMENT), LOAD_COMMIT_BODY);

const LOAD_REQUEST_BODY: ProgramSource<LoadRequestBody> = crate::g::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_STAGE, LoadBegin>,
        0,
    >(),
    LOAD_STREAM_BODY,
);

const LOAD_REQUEST: ProgramSource<LoadRequestArm> = g::seq(LOAD_REQUEST_HEAD, LOAD_REQUEST_BODY);

const LOAD_ACTIVATE_BODY: ProgramSource<LoadActivateBody> = crate::g::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>,
        0,
    >(),
    LOAD_STREAM_BODY,
);

const LOAD_ACTIVATE_REQUEST: ProgramSource<LoadActivateArm> =
    g::seq(LOAD_ACTIVATE_HEAD, LOAD_ACTIVATE_BODY);

const LOAD_FAMILY_HEAD: ProgramSource<
    ControllerHead<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_LOAD_FAMILY, MgmtRouteLoadFamilyKind>,
    0,
>()
.policy::<REQUEST_ROOT_POLICY_ID>();

const LOAD_ROUTE: ProgramSource<LoadRouteSteps> = g::route(LOAD_REQUEST, LOAD_ACTIVATE_REQUEST);
const LOAD_FAMILY_REQUEST: ProgramSource<LoadFamilyArm> = g::seq(LOAD_FAMILY_HEAD, LOAD_ROUTE);

type ActivateBody = ControllerSend<LABEL_MGMT_ACTIVATE, SlotRequest>;
type ActivateArm =
    SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>, ActivateBody>;

const ACTIVATE_HEAD: ProgramSource<
    ControllerHead<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_ACTIVATE, MgmtRouteActivateKind>,
    0,
>()
.policy::<REQUEST_COMMAND_POLICY_ID>();

const ACTIVATE_BODY: ProgramSource<ActivateBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_ACTIVATE, SlotRequest>,
    0,
>();

const ACTIVATE_REQUEST: ProgramSource<ActivateArm> = g::seq(ACTIVATE_HEAD, ACTIVATE_BODY);

type RevertBody = ControllerSend<LABEL_MGMT_REVERT, SlotRequest>;
type RevertArm = SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>, RevertBody>;

const REVERT_HEAD: ProgramSource<ControllerHead<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_REVERT, MgmtRouteRevertKind>,
        0,
    >()
    .policy::<REQUEST_COMMAND_TAIL_POLICY_ID>();

const REVERT_BODY: ProgramSource<RevertBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_REVERT, SlotRequest>,
    0,
>();

const REVERT_REQUEST: ProgramSource<RevertArm> = g::seq(REVERT_HEAD, REVERT_BODY);

type StatsBody = ControllerSend<LABEL_MGMT_STATS, SlotRequest>;
type StatsArm = SeqSteps<ControllerHead<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>, StatsBody>;

const STATS_HEAD: ProgramSource<ControllerHead<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>> =
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CONTROLLER>,
        RouteMsg<LABEL_MGMT_ROUTE_STATS, MgmtRouteStatsKind>,
        0,
    >()
    .policy::<REQUEST_COMMAND_TAIL_POLICY_ID>();

const STATS_BODY: ProgramSource<StatsBody> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_MGMT_STATS, SlotRequest>,
    0,
>();

const STATS_REQUEST: ProgramSource<StatsArm> = g::seq(STATS_HEAD, STATS_BODY);

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

const COMMAND_TAIL_HEAD: ProgramSource<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_COMMAND_TAIL, MgmtRouteCommandTailKind>,
    0,
>()
.policy::<REQUEST_COMMAND_POLICY_ID>();

const COMMAND_TAIL_ROUTE: ProgramSource<CommandTailRouteSteps> =
    g::route(REVERT_REQUEST, STATS_REQUEST);
const COMMAND_TAIL_REQUEST: ProgramSource<CommandTailArm> =
    g::seq(COMMAND_TAIL_HEAD, COMMAND_TAIL_ROUTE);

const COMMAND_ROUTE: ProgramSource<CommandRouteSteps> =
    g::route(ACTIVATE_REQUEST, COMMAND_TAIL_REQUEST);

const COMMAND_FAMILY_HEAD: ProgramSource<
    ControllerHead<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_COMMAND_FAMILY, MgmtRouteCommandFamilyKind>,
    0,
>()
.policy::<REQUEST_ROOT_POLICY_ID>();

const COMMAND_FAMILY_REQUEST: ProgramSource<CommandFamilyArm> =
    g::seq(COMMAND_FAMILY_HEAD, COMMAND_ROUTE);
const REQUEST_ROUTE: ProgramSource<RequestRouteSteps> =
    g::route(LOAD_FAMILY_REQUEST, COMMAND_FAMILY_REQUEST);

type ErrorReplyBody = ClusterSend<LABEL_MGMT_REPLY_ERROR, MgmtError>;
type ErrorReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>, ErrorReplyBody>;

const ERROR_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind>,
    0,
>()
.policy::<REPLY_ROOT_POLICY_ID>();

const ERROR_REPLY_BODY: ProgramSource<ErrorReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_ERROR, MgmtError>,
    0,
>();

const ERROR_REPLY: ProgramSource<ErrorReplyArm> = g::seq(ERROR_REPLY_HEAD, ERROR_REPLY_BODY);

type LoadedReplyBody = ClusterSend<LABEL_MGMT_REPLY_LOADED, LoadReport>;
type LoadedReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>, LoadedReplyBody>;

const LOADED_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_LOADED, MgmtRouteReplyLoadedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_POLICY_ID>();

const LOADED_REPLY_BODY: ProgramSource<LoadedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_LOADED, LoadReport>,
    0,
>();

const LOADED_REPLY: ProgramSource<LoadedReplyArm> = g::seq(LOADED_REPLY_HEAD, LOADED_REPLY_BODY);

type ActivatedReplyBody = ClusterSend<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>;
type ActivatedReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
    ActivatedReplyBody,
>;

const ACTIVATED_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_ACTIVATED, MgmtRouteReplyActivatedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_TAIL_POLICY_ID>();

const ACTIVATED_REPLY_BODY: ProgramSource<ActivatedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_ACTIVATED, super::TransitionReport>,
    0,
>();

const ACTIVATED_REPLY: ProgramSource<ActivatedReplyArm> =
    g::seq(ACTIVATED_REPLY_HEAD, ACTIVATED_REPLY_BODY);

type RevertedReplyBody = ClusterSend<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>;
type RevertedReplyArm = SeqSteps<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
    RevertedReplyBody,
>;

const REVERTED_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_REVERTED, MgmtRouteReplyRevertedKind>,
    0,
>()
.policy::<REPLY_SUCCESS_FINAL_POLICY_ID>();

const REVERTED_REPLY_BODY: ProgramSource<RevertedReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_REVERTED, super::TransitionReport>,
    0,
>();

const REVERTED_REPLY: ProgramSource<RevertedReplyArm> =
    g::seq(REVERTED_REPLY_HEAD, REVERTED_REPLY_BODY);

type StatsReplyBody = ClusterSend<LABEL_MGMT_REPLY_STATS, StatsReply>;
type StatsReplyArm =
    SeqSteps<ClusterHead<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>, StatsReplyBody>;

const STATS_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_STATS, MgmtRouteReplyStatsKind>,
    0,
>()
.policy::<REPLY_SUCCESS_FINAL_POLICY_ID>();

const STATS_REPLY_BODY: ProgramSource<StatsReplyBody> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_MGMT_REPLY_STATS, StatsReply>,
    0,
>();

const STATS_REPLY: ProgramSource<StatsReplyArm> = g::seq(STATS_REPLY_HEAD, STATS_REPLY_BODY);

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
pub type ProgramSteps = SeqSteps<RequestRouteSteps, ReplyRouteSteps>;

const SUCCESS_FINAL_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
    0,
>()
.policy::<REPLY_SUCCESS_TAIL_POLICY_ID>();

const SUCCESS_TAIL_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
    0,
>()
.policy::<REPLY_SUCCESS_POLICY_ID>();

const SUCCESS_REPLY_HEAD: ProgramSource<
    ClusterHead<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY, MgmtRouteReplySuccessFamilyKind>,
    0,
>()
.policy::<REPLY_ROOT_POLICY_ID>();

const SUCCESS_FINAL_REPLY_ROUTE: ProgramSource<SuccessFinalReplyRouteSteps> =
    g::route(REVERTED_REPLY, STATS_REPLY);
const SUCCESS_FINAL_REPLY_FAMILY: ProgramSource<SuccessFinalReplyArm> =
    g::seq(SUCCESS_FINAL_REPLY_HEAD, SUCCESS_FINAL_REPLY_ROUTE);
const SUCCESS_TAIL_REPLY_ROUTE: ProgramSource<SuccessTailReplyRouteSteps> =
    g::route(ACTIVATED_REPLY, SUCCESS_FINAL_REPLY_FAMILY);
const SUCCESS_TAIL_REPLY_FAMILY: ProgramSource<SuccessTailReplyArm> =
    g::seq(SUCCESS_TAIL_REPLY_HEAD, SUCCESS_TAIL_REPLY_ROUTE);
const SUCCESS_REPLY_ROUTE: ProgramSource<SuccessReplyRouteSteps> =
    g::route(LOADED_REPLY, SUCCESS_TAIL_REPLY_FAMILY);
const SUCCESS_REPLY_FAMILY: ProgramSource<SuccessReplyArm> =
    g::seq(SUCCESS_REPLY_HEAD, SUCCESS_REPLY_ROUTE);
const REPLY_ROUTE: ProgramSource<ReplyRouteSteps> = g::route(ERROR_REPLY, SUCCESS_REPLY_FAMILY);

pub const PROGRAM: ProgramSource<ProgramSteps> = crate::g::seq(REQUEST_ROUTE, REPLY_ROUTE);

#[cfg(test)]
pub(super) static CONTROLLER_PROGRAM: RoleProgram<'static, ROLE_CONTROLLER, ProgramSteps> =
    project(&g::freeze(&PROGRAM));

#[cfg(test)]
pub(super) static CLUSTER_PROGRAM: RoleProgram<'static, ROLE_CLUSTER, ProgramSteps> =
    project(&g::freeze(&PROGRAM));

const _: () = assert_program_covers_facets(&PROGRAM, MGMT_FACET_NEEDS);
