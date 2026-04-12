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
    g::{self, Program},
    global::{
        CanonicalControl, ExternalControl,
        steps::{self, PolicySteps, RouteSteps, SeqSteps, StepCons, StepNil},
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
type ControllerPolicyHead<const LABEL: u8, Kind, const POLICY_ID: u16> =
    PolicySteps<ControllerHead<LABEL, Kind>, POLICY_ID>;
type ClusterHead<const LABEL: u8, Kind> = StepCons<
    steps::SendStep<g::Role<ROLE_CLUSTER>, g::Role<ROLE_CLUSTER>, RouteMsg<LABEL, Kind>>,
    StepNil,
>;
type ClusterPolicyHead<const LABEL: u8, Kind, const POLICY_ID: u16> =
    PolicySteps<ClusterHead<LABEL, Kind>, POLICY_ID>;
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

const LOAD_REQUEST_HEAD: Program<
    ControllerPolicyHead<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind, REQUEST_LOAD_POLICY_ID>,
> = g::send::<
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
type LoopContinueHead = PolicySteps<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CONTROLLER>,
            g::Role<ROLE_CONTROLLER>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    LOOP_POLICY_ID,
>;
type LoopBreakHead = PolicySteps<
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
    LOOP_POLICY_ID,
>;
type LoopContinueArm = SeqSteps<LoopContinueHead, LoopContinueBody>;
type LoopBreakArm = SeqSteps<LoopBreakHead, LoadFinalChunkBody>;

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

const LOOP_BREAK_PREFIX: Program<LoopBreakHead> = g::send::<
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

type LoopSegmentSteps = RouteSteps<LoopContinueArm, LoopBreakArm>;
const LOOP_SEGMENT: Program<LoopSegmentSteps> = g::route(LOOP_CONTINUE_ARM, LOOP_BREAK_ARM);

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

type LoadStreamBody = SeqSteps<SeqSteps<LoadBeginTokenBody, LoopSegmentSteps>, LoadCommitBody>;
type LoadRequestBody = SeqSteps<ControllerSend<LABEL_MGMT_STAGE, LoadBegin>, LoadStreamBody>;
type LoadRequestArm = SeqSteps<
    ControllerPolicyHead<LABEL_MGMT_ROUTE_LOAD, MgmtRouteLoadKind, REQUEST_LOAD_POLICY_ID>,
    LoadRequestBody,
>;
type LoadActivateBody =
    SeqSteps<ControllerSend<LABEL_MGMT_LOAD_AND_ACTIVATE, LoadBegin>, LoadStreamBody>;
type LoadActivateArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE,
        MgmtRouteLoadAndActivateKind,
        REQUEST_LOAD_POLICY_ID,
    >,
    LoadActivateBody,
>;
type LoadRouteSteps = RouteSteps<LoadRequestArm, LoadActivateArm>;
type LoadFamilyArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_LOAD_FAMILY,
        MgmtRouteLoadFamilyKind,
        REQUEST_ROOT_POLICY_ID,
    >,
    LoadRouteSteps,
>;

const LOAD_ACTIVATE_HEAD: Program<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE,
        MgmtRouteLoadAndActivateKind,
        REQUEST_LOAD_POLICY_ID,
    >,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CONTROLLER>,
    RouteMsg<LABEL_MGMT_ROUTE_LOAD_AND_ACTIVATE, MgmtRouteLoadAndActivateKind>,
    0,
>()
.policy::<REQUEST_LOAD_POLICY_ID>();

const LOAD_STREAM_BODY: Program<LoadStreamBody> =
    crate::g::seq(crate::g::seq(LOAD_BEGIN, LOOP_SEGMENT), LOAD_COMMIT_BODY);

const LOAD_REQUEST_BODY: Program<LoadRequestBody> = crate::g::seq(
    g::send::<
        g::Role<ROLE_CONTROLLER>,
        g::Role<ROLE_CLUSTER>,
        g::Msg<LABEL_MGMT_STAGE, LoadBegin>,
        0,
    >(),
    LOAD_STREAM_BODY,
);

const LOAD_REQUEST: Program<LoadRequestArm> = g::seq(LOAD_REQUEST_HEAD, LOAD_REQUEST_BODY);

const LOAD_ACTIVATE_BODY: Program<LoadActivateBody> = crate::g::seq(
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
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_LOAD_FAMILY,
        MgmtRouteLoadFamilyKind,
        REQUEST_ROOT_POLICY_ID,
    >,
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
type ActivateArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_ACTIVATE,
        MgmtRouteActivateKind,
        REQUEST_COMMAND_POLICY_ID,
    >,
    ActivateBody,
>;

const ACTIVATE_HEAD: Program<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_ACTIVATE,
        MgmtRouteActivateKind,
        REQUEST_COMMAND_POLICY_ID,
    >,
> = g::send::<
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
type RevertArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_REVERT,
        MgmtRouteRevertKind,
        REQUEST_COMMAND_TAIL_POLICY_ID,
    >,
    RevertBody,
>;

const REVERT_HEAD: Program<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_REVERT,
        MgmtRouteRevertKind,
        REQUEST_COMMAND_TAIL_POLICY_ID,
    >,
> = g::send::<
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
type StatsArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_STATS,
        MgmtRouteStatsKind,
        REQUEST_COMMAND_TAIL_POLICY_ID,
    >,
    StatsBody,
>;

const STATS_HEAD: Program<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_STATS,
        MgmtRouteStatsKind,
        REQUEST_COMMAND_TAIL_POLICY_ID,
    >,
> = g::send::<
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

type CommandTailRouteSteps = RouteSteps<RevertArm, StatsArm>;
type CommandTailArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_COMMAND_TAIL,
        MgmtRouteCommandTailKind,
        REQUEST_COMMAND_POLICY_ID,
    >,
    CommandTailRouteSteps,
>;
type CommandRouteSteps = RouteSteps<ActivateArm, CommandTailArm>;
type CommandFamilyArm = SeqSteps<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_COMMAND_FAMILY,
        MgmtRouteCommandFamilyKind,
        REQUEST_ROOT_POLICY_ID,
    >,
    CommandRouteSteps,
>;
type RequestRouteSteps = RouteSteps<LoadFamilyArm, CommandFamilyArm>;

const COMMAND_TAIL_HEAD: Program<
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_COMMAND_TAIL,
        MgmtRouteCommandTailKind,
        REQUEST_COMMAND_POLICY_ID,
    >,
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
    ControllerPolicyHead<
        LABEL_MGMT_ROUTE_COMMAND_FAMILY,
        MgmtRouteCommandFamilyKind,
        REQUEST_ROOT_POLICY_ID,
    >,
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
type ErrorReplyArm = SeqSteps<
    ClusterPolicyHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind, REPLY_ROOT_POLICY_ID>,
    ErrorReplyBody,
>;

const ERROR_REPLY_HEAD: Program<
    ClusterPolicyHead<LABEL_MGMT_ROUTE_REPLY_ERROR, MgmtRouteReplyErrorKind, REPLY_ROOT_POLICY_ID>,
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
type LoadedReplyArm = SeqSteps<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_LOADED,
        MgmtRouteReplyLoadedKind,
        REPLY_SUCCESS_POLICY_ID,
    >,
    LoadedReplyBody,
>;

const LOADED_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_LOADED,
        MgmtRouteReplyLoadedKind,
        REPLY_SUCCESS_POLICY_ID,
    >,
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
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_ACTIVATED,
        MgmtRouteReplyActivatedKind,
        REPLY_SUCCESS_TAIL_POLICY_ID,
    >,
    ActivatedReplyBody,
>;

const ACTIVATED_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_ACTIVATED,
        MgmtRouteReplyActivatedKind,
        REPLY_SUCCESS_TAIL_POLICY_ID,
    >,
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
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_REVERTED,
        MgmtRouteReplyRevertedKind,
        REPLY_SUCCESS_FINAL_POLICY_ID,
    >,
    RevertedReplyBody,
>;

const REVERTED_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_REVERTED,
        MgmtRouteReplyRevertedKind,
        REPLY_SUCCESS_FINAL_POLICY_ID,
    >,
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
type StatsReplyArm = SeqSteps<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_STATS,
        MgmtRouteReplyStatsKind,
        REPLY_SUCCESS_FINAL_POLICY_ID,
    >,
    StatsReplyBody,
>;

const STATS_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_STATS,
        MgmtRouteReplyStatsKind,
        REPLY_SUCCESS_FINAL_POLICY_ID,
    >,
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

type SuccessFinalReplyRouteSteps = RouteSteps<RevertedReplyArm, StatsReplyArm>;
type SuccessFinalReplyArm = SeqSteps<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL,
        MgmtRouteReplySuccessFinalKind,
        REPLY_SUCCESS_TAIL_POLICY_ID,
    >,
    SuccessFinalReplyRouteSteps,
>;
type SuccessTailReplyRouteSteps = RouteSteps<ActivatedReplyArm, SuccessFinalReplyArm>;
type SuccessTailReplyArm = SeqSteps<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL,
        MgmtRouteReplySuccessTailKind,
        REPLY_SUCCESS_POLICY_ID,
    >,
    SuccessTailReplyRouteSteps,
>;
type SuccessReplyRouteSteps = RouteSteps<LoadedReplyArm, SuccessTailReplyArm>;
type SuccessReplyArm = SeqSteps<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY,
        MgmtRouteReplySuccessFamilyKind,
        REPLY_ROOT_POLICY_ID,
    >,
    SuccessReplyRouteSteps,
>;
type ReplyRouteSteps = RouteSteps<ErrorReplyArm, SuccessReplyArm>;
pub type ProgramSteps = SeqSteps<RequestRouteSteps, ReplyRouteSteps>;

const SUCCESS_FINAL_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL,
        MgmtRouteReplySuccessFinalKind,
        REPLY_SUCCESS_TAIL_POLICY_ID,
    >,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_FINAL, MgmtRouteReplySuccessFinalKind>,
    0,
>()
.policy::<REPLY_SUCCESS_TAIL_POLICY_ID>();

const SUCCESS_TAIL_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL,
        MgmtRouteReplySuccessTailKind,
        REPLY_SUCCESS_POLICY_ID,
    >,
> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    RouteMsg<LABEL_MGMT_ROUTE_REPLY_SUCCESS_TAIL, MgmtRouteReplySuccessTailKind>,
    0,
>()
.policy::<REPLY_SUCCESS_POLICY_ID>();

const SUCCESS_REPLY_HEAD: Program<
    ClusterPolicyHead<
        LABEL_MGMT_ROUTE_REPLY_SUCCESS_FAMILY,
        MgmtRouteReplySuccessFamilyKind,
        REPLY_ROOT_POLICY_ID,
    >,
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

pub const PROGRAM: Program<ProgramSteps> = crate::g::seq(REQUEST_ROUTE, REPLY_ROUTE);

#[cfg(test)]
pub(super) static CONTROLLER_PROGRAM: RoleProgram<'static, ROLE_CONTROLLER, ProgramSteps> =
    project(&PROGRAM);

#[cfg(test)]
pub(super) static CLUSTER_PROGRAM: RoleProgram<'static, ROLE_CLUSTER, ProgramSteps> =
    project(&PROGRAM);

const _: () = assert_program_covers_facets(&PROGRAM, MGMT_FACET_NEEDS);
