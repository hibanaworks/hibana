use super::{ActorEvent, ActorEventProjection, RoleLocalDirection};
use crate::global::compiled::{
    images::{CompiledProgramRef, ProgramImageBytes, ProgramImagePlan},
    lowering::CompiledProgramImage,
};
use crate::{Endpoint, g};

type Loan = g::Msg<1, Endpoint<'static, 255>>;
type OtherLoan = g::Msg<2, Endpoint<'static, 255>>;
type Steps = g::Seq<
    g::Resolve<g::Route<g::Send<0, 1, Loan>, g::Send<0, 2, OtherLoan>>, 701>,
    g::Send<255, 3, g::Msg<3, u32>>,
>;
const SOURCE: g::ProgramSourceData<32> = g::ProgramSourceData::lower::<Steps>();
const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
static BYTES: ProgramImageBytes<128> =
    match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
        Some(bytes) => bytes,
        None => panic!("actor projection proof storage"),
    };
const PROGRAM: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());

const ABSENT: u16 = u16::MAX;

#[derive(Clone, Copy)]
struct Observation {
    first: u16,
    second: u16,
    third: u16,
}

const fn encode(row: Option<ActorEvent>) -> u16 {
    match row {
        Some(row) => {
            let side = match row.direction {
                RoleLocalDirection::Local => 0,
                RoleLocalDirection::Send => 1,
                RoleLocalDirection::Recv => 2,
            };
            row.event as u16 * 4 + side
        }
        None => ABSENT,
    }
}

// Evaluate the production iterator for the complete closed u8 actor domain.
// CBMC then checks each independently stated property for a symbolic table
// index. This avoids duplicating packed-image parsing at every loop unwind.
// No actor, safety check, or possible third operation is excluded.
const OBSERVATIONS: [Observation; 256] = {
    let mut observations = [Observation {
        first: ABSENT,
        second: ABSENT,
        third: ABSENT,
    }; 256];
    let mut actor = 0;
    while actor < 256 {
        let mut scratch = [0xff];
        let Some(mut projection) = ActorEventProjection::new(&PROGRAM, actor as u8, &mut scratch)
        else {
            panic!("actor projection proof scratch");
        };
        observations[actor] = Observation {
            first: encode(projection.next()),
            second: encode(projection.next()),
            third: encode(projection.next()),
        };
        actor += 1;
    }
    observations
};

#[kani::proof]
#[kani::unwind(16)]
fn actor_projection_emits_the_exact_count_for_every_actor() {
    let actor: u8 = kani::any();
    let observation = OBSERVATIONS[actor as usize];
    assert!(observation.third == ABSENT);
    let count =
        usize::from(observation.first != ABSENT) + usize::from(observation.second != ABSENT);
    let expected = match actor {
        0..=2 => 2,
        3 => 1,
        4..=255 => 0,
    };
    assert!(count == expected);
}

#[kani::proof]
#[kani::unwind(16)]
fn actor_projection_emits_only_permitted_sides_for_every_actor() {
    let actor: u8 = kani::any();
    let observation = OBSERVATIONS[actor as usize];
    assert!(observation.third == ABSENT);
    if observation.first != ABSENT {
        assert_permitted(actor, observation.first);
    }
    if observation.second != ABSENT {
        assert_permitted(actor, observation.second);
    }
}

fn assert_permitted(actor: u8, row: u16) {
    let permitted = match row {
        1 | 5 => actor == 0,
        2 => actor == 1,
        6 => actor == 2,
        9 => actor == 1 || actor == 2,
        10 => actor == 3,
        _ => false,
    };
    assert!(permitted);
}

#[kani::proof]
#[kani::unwind(16)]
fn actor_projection_emits_each_side_once_in_storage_order() {
    let actor: u8 = kani::any();
    let observation = OBSERVATIONS[actor as usize];
    assert!(observation.third == ABSENT);
    if observation.first != ABSENT && observation.second != ABSENT {
        assert!(observation.first < observation.second);
    }
}
