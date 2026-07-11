use super::{LocalNode, PackedEventConflict, PackedLocalDependency};

#[kani::proof]
fn packed_event_conflict_decoding_accepts_exact_domain() {
    let raw: u16 = kani::any();
    let decoded = PackedEventConflict::decode_raw(raw);
    let route_ordinal = (raw >> 1) & 0x0fff;
    let route = (raw & !0x3fff) == 0 && (route_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES;
    let valid = route || raw == 0x6000 || raw == 0xffff;

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
        && end <= 0x0fff
        && (dep_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES
        && (conflict_route & 0x8000) == 0
        && (if conflict_tag >= 2 {
            (route_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES
        } else {
            route_ordinal == 0
        });

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
        && end <= 0x0fff
        && end <= event_count
        && (dep_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES
        && (conflict_route & 0x8000) == 0
        && (if conflict_tag >= 2 {
            (route_ordinal as usize) < crate::eff::meta::MAX_EFF_NODES
        } else {
            route_ordinal == 0
        });

    assert!(decoded.is_some() == (absent || present));
    assert!(matches!(decoded, Some(None)) == absent);
    if let Some(Some(dependency)) = decoded {
        assert!(dependency.end() <= event_count as usize);
    }
}
