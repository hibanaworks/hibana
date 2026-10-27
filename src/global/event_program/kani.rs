use super::LocalEventProgram;
use crate::{g, global::const_dsl::ScopeId};

#[kani::proof]
#[kani::unwind(12)]
fn binary_roll_lookup_matches_all_rows_and_absent_identifiers() {
    let global = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>().roll(),
        g::send::<0, 1, g::Msg<2, ()>>().roll().roll(),
    );
    let program = crate::runtime::program::project::<0>(&global);
    let events = LocalEventProgram::from_rows(program.role_image_ref());
    assert_eq!(events.footprint().roll_scope_count, 3);
    let ordinal = kani::any::<u16>() % ScopeId::LOCAL_CAPACITY;
    let scope = ScopeId::roll_scope(ordinal);
    let mut expected = None;
    for slot in 0..3 {
        let (candidate, _) = events.roll_scope_row_by_slot(slot).unwrap();
        if candidate == scope {
            expected = Some(slot);
        }
    }
    assert_eq!(events.roll_scope_slot(scope), expected);
    assert_eq!(events.roll_scope_slot(ScopeId::route(ordinal)), None);
    assert_eq!(events.roll_scope_slot(ScopeId::none()), None);
    kani::cover!(expected == Some(0));
    kani::cover!(expected == Some(1));
    kani::cover!(expected == Some(2));
    kani::cover!(ordinal == ScopeId::LOCAL_CAPACITY - 1 && expected.is_none());
}
