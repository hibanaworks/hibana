use hibana::{
    g,
    g::advanced::{
        project,
        steps::{SendStep, SeqSteps, StepCons, StepNil},
    },
    substrate::{
        cap::advanced::CapsMask, cap::advanced::MintConfig, policy::PolicySlot, tap::TapEvent,
        transport::TransportSnapshot,
    },
};
use hibana_epf::{Action, Header, HostSlots, ScratchLease, Slot, loader::ImageLoader, run_with};
use hibana_mgmt::{
    LoadRequest, ROLE_CLUSTER, ROLE_CONTROLLER, Request, SubscribeReq, observe_stream,
    request_reply,
};

type MgmtAppSteps =
    StepCons<SendStep<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CLUSTER>, g::Msg<120, u32>>, StepNil>;
type MgmtProgramSteps = SeqSteps<request_reply::ProgramSteps, MgmtAppSteps>;
const MGMT_APP: g::Program<MgmtAppSteps> =
    g::send::<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CLUSTER>, g::Msg<120, u32>, 0>();
const MGMT_PROGRAM: g::Program<MgmtProgramSteps> = g::seq(request_reply::PROGRAM, MGMT_APP);

type ObserveAppSteps =
    StepCons<SendStep<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CLUSTER>, g::Msg<121, ()>>, StepNil>;
type ObserveProgramSteps = SeqSteps<observe_stream::ProgramSteps, ObserveAppSteps>;
const OBSERVE_APP: g::Program<ObserveAppSteps> =
    g::send::<g::Role<ROLE_CONTROLLER>, g::Role<ROLE_CLUSTER>, g::Msg<121, ()>, 0>();
const OBSERVE_PROGRAM: g::Program<ObserveProgramSteps> =
    g::seq(observe_stream::PROGRAM, OBSERVE_APP);

fn header_for(code: &[u8], mem_len: u16) -> Header {
    Header {
        code_len: code.len() as u16,
        fuel_max: 8,
        mem_len,
        flags: 0,
        hash: hibana_epf::verifier::compute_hash(code),
    }
}

#[test]
fn request_reply_program_projects_from_external_crate_context() {
    let _controller: hibana::g::advanced::RoleProgram<'_, ROLE_CONTROLLER, MintConfig> =
        project(&MGMT_PROGRAM);
    let _cluster: hibana::g::advanced::RoleProgram<'_, ROLE_CLUSTER, MintConfig> =
        project(&MGMT_PROGRAM);

    let _request = Request::LoadAndActivate(LoadRequest {
        slot: PolicySlot::Route,
        code: &[0x30, 0x03, 0x00, 0x01],
        fuel_max: 64,
        mem_len: 128,
    });
}

#[test]
fn observe_stream_program_projects_from_external_crate_context() {
    let _controller: hibana::g::advanced::RoleProgram<'_, ROLE_CONTROLLER, MintConfig> =
        project(&OBSERVE_PROGRAM);
    let _cluster: hibana::g::advanced::RoleProgram<'_, ROLE_CLUSTER, MintConfig> =
        project(&OBSERVE_PROGRAM);

    let _subscribe = SubscribeReq::default();
    let _tap = TapEvent::default();
}

#[test]
fn epf_runtime_executes_under_split_repo_dependency_shape() {
    let code = [0x41, 0x00, 0x33, 0x00];
    let mut loader = ImageLoader::new();
    loader.begin(header_for(&code, 16)).expect("begin");
    loader.write(0, &code).expect("write");
    let verified = loader.commit_for_slot(Slot::Route).expect("verify");

    let mut slots = HostSlots::new();
    let mut scratch = [0u8; 16];
    slots
        .install_verified(Slot::Route, verified, ScratchLease::new(&mut scratch))
        .expect("install");

    let action = run_with(
        &slots,
        Slot::Route,
        &TapEvent::zero(),
        CapsMask::allow_all(),
        None,
        None,
        |ctx| ctx.set_transport_snapshot(TransportSnapshot::new(None, Some(3))),
    );
    assert_eq!(action, Action::Route { arm: 3 });
}
