use super::{LocalNode, PackedEventConflict, PackedLocalDependency, StateIndex};

#[kani::proof]
fn state_index_preserves_the_exact_present_identity_domain() {
    let raw: u16 = kani::any();
    kani::assume(raw != u16::MAX);

    let state = StateIndex::new(raw);

    assert_eq!(state.raw(), raw);
    assert!(!state.is_absent());
}

#[kani::proof]
#[kani::should_panic]
fn state_index_rejects_the_reserved_absent_identity() {
    let _ = StateIndex::from_usize(crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY);
}

#[kani::proof]
fn packed_event_conflict_decoding_accepts_exact_domain() {
    let raw: u16 = kani::any();
    let decoded = PackedEventConflict::decode_raw(raw);
    let route_ordinal = (raw >> 1) & 0x1fff;
    let route = (raw & 0x8000) == 0;
    let valid = route || raw == 0xc000 || raw == 0xffff;

    assert!(decoded.is_some() == valid);
    assert!(decoded.is_some_and(|conflict| !conflict.is_none()) == route);
    if let Some(conflict) = decoded {
        assert!(conflict.raw() == raw);
        match (route, conflict.decoded_route()) {
            (true, Some((ordinal, arm))) => {
                assert!(ordinal == route_ordinal);
                assert!(arm == (raw & 1) as u8);
            }
            (false, None) => {}
            _ => assert!(false),
        }
    }
}

#[kani::proof]
fn optional_route_arm_decoding_accepts_exact_domain() {
    let raw: u8 = kani::any();
    let decoded = LocalNode::decode_optional_route_arm_raw(raw);

    match (raw, decoded) {
        (0, Some(Some(0))) | (1, Some(Some(1))) | (255, Some(None)) => {}
        (2..=254, None) => {}
        _ => assert!(false),
    }
}

#[kani::proof]
fn packed_local_dependency_decoding_accepts_exact_domain() {
    let start: u16 = kani::any();
    let end: u16 = kani::any();
    let dep_ordinal: u16 = kani::any();
    let conflict_route: u16 = kani::any();
    let packed = PackedLocalDependency::from_packed_parts(start, end, dep_ordinal, conflict_route);
    let decoded = packed.decode();
    let absent = start == u16::MAX
        && end == u16::MAX
        && dep_ordinal == u16::MAX
        && conflict_route == u16::MAX;
    let conflict_tag = conflict_route & 0b11;
    let route_ordinal = conflict_route >> 2;
    let present = start < end
        && dep_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY
        && (conflict_route & 0x8000) == 0
        && (if conflict_tag >= 2 {
            route_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY
        } else {
            route_ordinal == 0
        });

    kani::cover!(present && start <= 4095 && end > 4095);
    kani::cover!(present && end == u16::MAX);
    assert!(decoded.is_some() == (absent || present));
    assert!(matches!(decoded, Some(None)) == absent);
    if let Some(Some(dependency)) = decoded {
        assert!(PackedLocalDependency::from_dependency(dependency) == packed);
    }
}

#[kani::proof]
fn packed_local_dependency_event_bounds_are_exact() {
    let start: u16 = kani::any();
    let end: u16 = kani::any();
    let dep_ordinal: u16 = kani::any();
    let conflict_route: u16 = kani::any();
    let event_count: u16 = kani::any();
    let packed = PackedLocalDependency::from_packed_parts(start, end, dep_ordinal, conflict_route);
    let decoded = packed.decode_for_event_count(event_count as usize);
    let absent = start == u16::MAX
        && end == u16::MAX
        && dep_ordinal == u16::MAX
        && conflict_route == u16::MAX;
    let conflict_tag = conflict_route & 0b11;
    let route_ordinal = conflict_route >> 2;
    let present = start < end
        && end <= event_count
        && dep_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY
        && (conflict_route & 0x8000) == 0
        && (if conflict_tag >= 2 {
            route_ordinal < crate::global::const_dsl::ScopeId::LOCAL_CAPACITY
        } else {
            route_ordinal == 0
        });

    kani::cover!(present && start <= 4095 && end > 4095);
    assert!(decoded.is_some() == (absent || present));
    assert!(matches!(decoded, Some(None)) == absent);
    if let Some(Some(dependency)) = decoded {
        assert!(dependency.end() <= event_count as usize);
    }
}
