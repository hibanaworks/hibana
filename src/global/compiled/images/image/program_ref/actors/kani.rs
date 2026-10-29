use super::super::CompiledProgramRef;
use super::fixtures::{COLUMNS, OWNERSHIP, PROGRAM, encoded_bytes};
use crate::global::const_dsl::{ScopeId, ScopeKind};

#[kani::proof]
#[kani::unwind(16)]
fn source_route_visibility_matches_the_complete_borrow_range() {
    use crate::{
        g,
        global::compiled::{
            images::{ProgramImageBytes, ProgramImagePlan},
            lowering::CompiledProgramImage,
        },
    };
    type Step = g::Send<0, 1, g::Msg<1, ()>>;
    type Steps = g::Route<g::Seq<g::Route<Step, Step>, Step>, Step>;
    const SOURCE: g::ProgramSourceData<16> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<256> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("route visibility fixture storage"),
        };
    const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    let start: usize = kani::any();
    let end: usize = kani::any();
    let bounded = start < end && end <= 4;
    assert_eq!(
        IMAGE.source_route_visible_from_range(ScopeId::route(0), start, end),
        bounded && (end <= 3 || 3 <= start || (start == 0 && end == 4))
    );
    assert_eq!(
        IMAGE.source_route_visible_from_range(ScopeId::route(1), start, end),
        bounded && (end <= 1 || (1 <= start && end <= 2) || (start == 0 && 2 <= end))
    );
    assert!(!IMAGE.source_route_visible_from_range(ScopeId::none(), start, end));
    assert!(!IMAGE.source_route_visible_from_range(ScopeId::parallel(0), start, end));
}

#[kani::proof]
#[kani::unwind(4)]
fn selected_actor_preserves_identity_and_requires_the_actual_choice() {
    let actor: u8 = kani::any();
    let right: bool = kani::any();
    assert_eq!(
        PROGRAM.selected_actor(u16::from(actor), &mut |_| None),
        Some(actor)
    );
    assert_eq!(
        PROGRAM.selected_actor(256, &mut |scope| {
            assert_eq!(scope, ScopeId::route(0));
            Some(u8::from(right))
        }),
        Some(if right { 2 } else { 1 })
    );
    assert_eq!(PROGRAM.selected_actor(256, &mut |_| None), None);
}

#[kani::proof]
#[kani::unwind(4)]
fn initial_issuance_lookup_preserves_the_complete_role_domain() {
    let role: u8 = kani::any();
    assert_eq!(
        PROGRAM.initial_actor_for(role),
        if role == 255 { Some(0) } else { None }
    );
}

#[kani::proof]
fn actor_row_lookups_reject_every_out_of_range_index() {
    let index: usize = kani::any();
    let expected = match index {
        0 => Some([0, 1]),
        1 => Some([0, 2]),
        2 => Some([256, 3]),
        _ => None,
    };
    assert_eq!(PROGRAM.actor_binding_at(index), expected);
    assert_eq!(
        PROGRAM.actor_decision_at(index),
        if index == 0 {
            Some((ScopeId::new(ScopeKind::Route, 0), [1, 2]))
        } else {
            None
        }
    );
}

#[kani::proof]
#[kani::should_panic]
// Atom decoding reads all nine bytes before it can reject the decision.
#[kani::unwind(16)]
fn owned_image_construction_rejects_a_self_referencing_decision() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let left = OWNERSHIP.decisions.offset as usize + 2;
        bytes[left] = 0;
        bytes[left + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

// Four source events and nine markers bound each scan by nine iterations.
// Keep the six in-range queries in separate solver instances. The complement
// below retains every other usize pair, including both maximum-index covers.
mod source_order_fixture {
    use super::CompiledProgramRef;
    use crate::global::compiled::{
        images::{ProgramImageBytes, ProgramImagePlan, SourceOrderIndex},
        lowering::CompiledProgramImage,
    };
    use crate::{Endpoint, g};
    type Grant = g::Msg<1, Endpoint<'static, 255>>;
    type Work = g::Msg<2, ()>;
    type Alternative = g::Msg<3, ()>;
    type Steps = g::Seq<
        g::Par<g::Send<0, 1, Grant>, g::Send<2, 3, Work>>,
        g::Roll<g::Route<g::Send<1, 2, Work>, g::Send<1, 2, Alternative>>>,
    >;
    // Four atoms plus nine scope markers fit the sixteen-row source bucket.
    const SOURCE: g::ProgramSourceData<16> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<256> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("source order fixture storage"),
        };
    pub(super) const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    pub(super) const INDEX: SourceOrderIndex<16> = match SourceOrderIndex::from_program(&IMAGE) {
        Some(index) => index,
        None => panic!("source order index"),
    };
    pub(super) fn check_pair<const BEFORE: usize, const AFTER: usize>() {
        let actual = IMAGE.source_precedes(BEFORE, AFTER);
        assert_eq!(actual, BEFORE < 2 && (AFTER == 2 || AFTER == 3));
        assert_eq!(INDEX.before(BEFORE, AFTER), actual);
        assert_eq!(IMAGE.source_precedes_scan(BEFORE, AFTER), actual);
        if let Some(cut) = IMAGE.source_predecessor_cut(BEFORE, AFTER) {
            assert!(cut <= BEFORE);
            for omitted in cut..=BEFORE {
                assert!(!IMAGE.source_precedes_scan(omitted, AFTER));
            }
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn source_roll_ancestors_preserve_membership_over_the_full_event_domain() {
    let image = &source_order_fixture::IMAGE;
    let event: usize = kani::any();
    let mut ancestors = image.source_roll_ancestors(event);
    if event == 2 || event == 3 {
        assert_eq!(
            ancestors.next(),
            Some(crate::global::const_dsl::ScopeId::roll_scope(1))
        );
        assert_eq!(ancestors.next(), None);
    } else {
        assert_eq!(ancestors.next(), None);
    }
    kani::cover!(event == 2);
    kani::cover!(event == usize::MAX);
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_0_1() {
    source_order_fixture::check_pair::<0, 1>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_0_2() {
    source_order_fixture::check_pair::<0, 2>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_0_3() {
    source_order_fixture::check_pair::<0, 3>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_1_2() {
    source_order_fixture::check_pair::<1, 2>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_1_3() {
    source_order_fixture::check_pair::<1, 3>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll_2_3() {
    source_order_fixture::check_pair::<2, 3>();
}

#[kani::proof]
#[kani::unwind(10)]
fn source_order_keeps_parallel_and_route_arms_independent_through_roll() {
    use source_order_fixture::{IMAGE, INDEX};
    let first: usize = kani::any();
    let second: usize = kani::any();
    let events = IMAGE.columns.atom_count();
    assert_eq!(events, 4);
    if first >= second || second >= events {
        assert!(!IMAGE.source_precedes(first, second));
        assert!(!INDEX.before(first, second));
        kani::cover!(first == usize::MAX);
        kani::cover!(second == usize::MAX);
    } else {
        assert!(matches!(
            (first, second),
            (0, 1) | (0, 2) | (0, 3) | (1, 2) | (1, 3) | (2, 3)
        ));
    }
}
