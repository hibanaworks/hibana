use crate::common::TestTransport;
use hibana::{
    control::{
        cluster::{DynamicResolution, ResolverContext},
        types::{LaneId, RendezvousId, SessionId},
    },
    global::const_dsl::DynamicMeta,
    rendezvous::{Lane, SessionId as RendezvousSessionId},
    runtime::{SessionCluster, config::CounterClock, consts::DefaultLabelUniverse, mgmt::session},
};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

type Cluster = SessionCluster<'static, TestTransport, DefaultLabelUniverse, CounterClock, 4>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LoopKey {
    rv: RendezvousId,
    session: SessionId,
    lane: LaneId,
}

impl LoopKey {
    const fn new(rv: RendezvousId, session: SessionId, lane: LaneId) -> Self {
        Self { rv, session, lane }
    }
}

#[derive(Clone, Copy, Debug)]
struct LoopSchedule {
    total: usize,
    sent: usize,
}

static MGMT_LOOP_SCHEDULE: LazyLock<Mutex<HashMap<LoopKey, LoopSchedule>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_mgmt_loop_resolvers(cluster: &Cluster, rv_id: RendezvousId) {
    for info in session::CONTROLLER_PROGRAM.control_plans() {
        if info.plan.is_dynamic() {
            cluster
                .register_control_plan_resolver(rv_id, &info, mgmt_loop_resolver)
                .expect("register management loop resolver");
        }
    }
}

pub fn reset_mgmt_loop_resolver(
    rv_id: RendezvousId,
    sid: RendezvousSessionId,
    lane: Lane,
    total_chunks: usize,
) {
    assert!(
        total_chunks > 0,
        "controller plan must include at least one chunk"
    );
    let key = LoopKey::new(rv_id, SessionId::new(sid.raw()), LaneId::new(lane.raw()));
    let mut schedules = MGMT_LOOP_SCHEDULE.lock().expect("lock mgmt loop schedules");
    schedules.insert(
        key,
        LoopSchedule {
            total: total_chunks,
            sent: 0,
        },
    );
}

fn mgmt_loop_resolver(
    _cluster: &Cluster,
    _meta: &DynamicMeta,
    ctx: ResolverContext,
) -> Result<DynamicResolution, ()> {
    let session = ctx.session.ok_or(())?;
    let key = LoopKey::new(ctx.rv_id, session, ctx.lane);
    let mut schedules = MGMT_LOOP_SCHEDULE.lock().expect("lock mgmt loop schedules");
    let schedule = schedules.get_mut(&key).ok_or(())?;
    if schedule.sent >= schedule.total {
        return Err(());
    }
    let decision = schedule.sent + 1 < schedule.total;
    schedule.sent += 1;
    if schedule.sent == schedule.total {
        schedules.remove(&key);
    }
    Ok(DynamicResolution::Loop { decision })
}
