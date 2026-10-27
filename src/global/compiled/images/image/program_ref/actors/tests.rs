use super::{
    super::CompiledProgramRef,
    fixtures::{COLUMNS, OWNERSHIP, PROGRAM, SOURCE, SUMMARY, encoded_bytes},
};
use crate::global::{
    compiled::images::{ProgramImageBytes, ProgramImagePlan},
    const_dsl::{ScopeId, ScopeKind},
};

mod source_order;

#[test]
fn packed_repetition_heads_and_parent_links_are_checked_before_publication() {
    use crate::{g, global::compiled::lowering::CompiledProgramImage};
    type Step = g::Send<0, 1, g::Msg<1, ()>>;
    type Steps = g::Seq<g::Roll<Step>, g::Roll<g::Roll<Step>>>;
    const SOURCE: g::ProgramSourceData<16> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<256> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("repetition index fixture storage"),
        };
    const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    const fn changed(offset: usize) -> [u8; 256] {
        let mut bytes = [0; 256];
        let base = PLAN.columns().blob_len();
        let mut index = 0;
        while index < base {
            bytes[index] = IMAGE.byte_at(index);
            index += 1;
        }
        index = 0;
        while index < IMAGE.source_order.len() {
            bytes[base + index] = IMAGE.source_order.byte_at(index);
            index += 1;
        }
        bytes[base + offset] ^= 1;
        bytes
    }
    static BAD_ROLL_HEAD: [u8; 256] = changed(3 * 9 + 2 * 2);
    static BAD_BINARY_HEAD: [u8; 256] = changed(3 * 9);
    static BAD_PARENT: [u8; 256] = changed(2 * 9 + 6);
    assert_eq!(IMAGE.source_order.len(), 3 * 9 + 2 * 4);
    for bytes in [&BAD_ROLL_HEAD, &BAD_BINARY_HEAD, &BAD_PARENT] {
        assert!(
            std::panic::catch_unwind(|| {
                CompiledProgramRef::compact(IMAGE.facts, PLAN.columns(), bytes)
            })
            .is_err()
        );
    }
}

#[test]
fn source_order_offsets_preserve_the_full_scope_identifier_domain() {
    use crate::eff::{EffAtom, EventOrigin, EventPayload};
    use crate::global::{
        compiled::{images::PROGRAM_IMAGE_STORAGE_BYTE_CAPACITY, lowering::CompiledProgramImage},
        const_dsl::EffList,
    };
    const SOURCE: EffList<5> = {
        let mut source = EffList::new_partitioned(2, 3, 0);
        let atom = EffAtom {
            from: 0,
            to: 1,
            label: 1,
            payload: EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        };
        source.push_event_mut(atom);
        source.push_event_mut(atom);
        source.push_parallel_scope_mut(ScopeId::parallel(ScopeId::LOCAL_CAPACITY - 1), 0, 1, 2);
        source
    };
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_program(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(&SOURCE);
    static BYTES: ProgramImageBytes<PROGRAM_IMAGE_STORAGE_BYTE_CAPACITY> =
        ProgramImageBytes::from_image(&SOURCE, PLAN.columns());
    const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    assert!(IMAGE.source_order.len() > u16::MAX as usize);
    assert_eq!(IMAGE.source_parallel_at(0), Some((0, 1, 2)));
    assert!(!IMAGE.source_precedes(0, 1));
    assert_eq!(
        IMAGE.source_precedes(0, 1),
        IMAGE.source_precedes_scan(0, 1)
    );
    assert_eq!(
        IMAGE.source_route_bounds(ScopeId::route(ScopeId::LOCAL_CAPACITY - 1)),
        None
    );
    assert!(!IMAGE.source_precedes(0, usize::MAX));
}

#[test]
fn packed_source_order_checks_ranges_parents_kinds_and_event_heads() {
    static VALID: [u8; 128] = encoded_bytes();
    assert!(PROGRAM.same_image(&CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &VALID)));
    const fn changed(offset: usize) -> [u8; 128] {
        let mut bytes = [0; 128];
        let mut at = 0;
        while at < COLUMNS.blob_len() {
            bytes[at] = PROGRAM.byte_at(at);
            at += 1;
        }
        let mut index = 0;
        while index < PROGRAM.source_order.len() {
            bytes[at + index] = PROGRAM.source_order.byte_at(index);
            index += 1;
        }
        bytes[at + offset] ^= 1;
        bytes
    }
    static BAD_START: [u8; 128] = changed(0);
    static BAD_SPLIT: [u8; 128] = changed(2);
    static BAD_END: [u8; 128] = changed(4);
    static BAD_PARENT: [u8; 128] = changed(6);
    static BAD_KIND: [u8; 128] = changed(8);
    static BAD_HEAD: [u8; 128] = changed(9);
    for bytes in [
        &BAD_START,
        &BAD_SPLIT,
        &BAD_END,
        &BAD_PARENT,
        &BAD_KIND,
        &BAD_HEAD,
    ] {
        assert!(
            std::panic::catch_unwind(|| CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, bytes))
                .is_err()
        );
    }
}

#[test]
fn a_shared_route_preview_is_visible_inside_an_arm_or_around_the_whole_choice() {
    use crate::{g, global::compiled::lowering::CompiledProgramImage};
    type Step = g::Send<0, 1, g::Msg<1, ()>>;
    type Steps = g::Route<g::Seq<g::Route<Step, Step>, Step>, Step>;
    const SOURCE: g::ProgramSourceData<32> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<256> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("route range fixture storage"),
        };
    const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    assert!(!IMAGE.has_actor_bindings());
    let markers = SOURCE.eff_list().scope_markers();
    for (scope, left, split, right) in [
        (markers.at(0).scope_id, 0, 3, 4),
        (markers.at(1).scope_id, 0, 1, 2),
    ] {
        for start in 0..=5 {
            for end in 0..=5 {
                let visible = start < end
                    && end <= 4
                    && ((left <= start && end <= split)
                        || (split <= start && end <= right)
                        || (start <= left && right <= end));
                assert_eq!(
                    IMAGE.source_route_visible_from_range(scope, start, end),
                    visible
                );
            }
        }
        assert!(!IMAGE.source_route_visible_from_range(scope, 0, usize::MAX));
    }
    assert!(!IMAGE.source_route_visible_from_range(ScopeId::none(), 0, 1));
    assert!(!IMAGE.source_route_visible_from_range(ScopeId::parallel(0), 0, 1));
}

#[test]
fn source_order_matches_the_typed_tree_at_nested_and_adjacent_boundaries() {
    use crate::{
        g,
        global::compiled::{images::SourceOrderIndex, lowering::CompiledProgramImage},
    };

    type Message = g::Msg<2, ()>;
    type Step = g::Send<0, 1, Message>;
    type Pair = g::Seq<Step, Step>;
    type Choice = g::Resolve<g::Route<Pair, Step>, 11>;
    type Steps = g::Seq<
        g::Par<g::Seq<Choice, Step>, g::Roll<g::Par<Step, Pair>>>,
        g::Seq<Choice, g::Par<Step, Step>>,
    >;
    const SOURCE: g::ProgramSourceData<64> = g::ProgramSourceData::lower::<Steps>();
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_source(&SOURCE);
    const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<512> =
        match ProgramImageBytes::from_source_if_fits(&SOURCE, PLAN.columns()) {
            Some(bytes) => bytes,
            None => panic!("nested source order fixture storage"),
        };
    const IMAGE: CompiledProgramRef = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
    const INDEX: SourceOrderIndex<64> = match SourceOrderIndex::from_program(&IMAGE) {
        Some(index) => index,
        None => panic!("nested source order index"),
    };
    let tree = &<Steps as g::ProgramShape>::SOURCE_NODE;
    let markers = SOURCE.eff_list().scope_markers();
    for offset in 0..=tree.event_count() {
        let expected: std::vec::Vec<_> = (0..markers.len())
            .filter(|&index| markers.at(index).offset() == offset)
            .collect();
        assert_eq!(
            IMAGE
                .source_markers_at(offset)
                .collect::<std::vec::Vec<_>>(),
            expected
        );
    }
    assert!(IMAGE.source_markers_at(usize::MAX).is_empty());
    for before in 0..tree.event_count() {
        for after in 0..tree.event_count() {
            assert_eq!(
                IMAGE.source_precedes(before, after),
                tree.before(before, after),
                "source pair {before} -> {after}"
            );
            assert_eq!(INDEX.before(before, after), tree.before(before, after));
        }
        assert!(!IMAGE.source_precedes(before, tree.event_count()));
        assert!(!IMAGE.source_precedes(usize::MAX, before));
        assert!(!INDEX.before(before, tree.event_count()));
        assert!(!INDEX.before(usize::MAX, before));
    }
}

#[test]
fn actor_selection_uses_the_exact_choice_and_never_guesses_an_unknown_holder() {
    for actor in 0..=u8::MAX {
        assert_eq!(
            PROGRAM.selected_actor(u16::from(actor), &mut |_| None),
            Some(actor)
        );
    }
    for arm in 0..=1 {
        assert_eq!(
            PROGRAM.selected_actor(256, &mut |scope| {
                assert_eq!(scope, ScopeId::route(0));
                Some(arm)
            }),
            Some(arm + 1)
        );
    }
    assert_eq!(PROGRAM.selected_actor(256, &mut |_| None), None);
}

#[test]
fn owned_count_coverage_reads_the_checked_header_and_rejects_drift() {
    let events = SOURCE.eff_list().len();
    let scopes = SOURCE.eff_list().scope_markers().len();
    let resolvers = COLUMNS.route_resolver_count();
    assert!(PROGRAM.covers_source_counts(events, scopes, 0));
    assert!(PROGRAM.covers_source_counts(events, scopes, resolvers));
    assert!(!COLUMNS.covers_source_counts(events, scopes, resolvers));
    assert!(!PROGRAM.covers_source_counts(events + 1, scopes, resolvers));
    assert!(!PROGRAM.covers_source_counts(events, scopes + 1, resolvers));
    assert!(!PROGRAM.covers_source_counts(events, scopes, resolvers + 1));
    assert!(!PROGRAM.covers_source_counts(usize::MAX, scopes, resolvers));
}

#[test]
fn owned_image_uses_the_existing_eight_byte_columns_and_exact_tail() {
    assert_eq!(core::mem::size_of_val(&COLUMNS), 8);
    assert!(PROGRAM.has_actor_bindings());
    assert_eq!(PROGRAM.actor_binding_at(0), Some([0, 1]));
    assert_eq!(PROGRAM.actor_binding_at(1), Some([0, 2]));
    assert_eq!(PROGRAM.actor_binding_at(2), Some([256, 3]));
    assert_eq!(
        PROGRAM.actor_decision_at(0),
        Some((ScopeId::new(ScopeKind::Route, 0), [1, 2]))
    );
    for role in 0..=u8::MAX {
        assert_eq!(
            PROGRAM.initial_actor_for(role),
            if role == 255 { Some(0) } else { None }
        );
    }
    assert!(ProgramImageBytes::<8>::from_source_if_fits(&SOURCE, COLUMNS).is_none());
    assert_eq!(OWNERSHIP.initial.end_offset(2), COLUMNS.blob_len());
}

#[test]
#[should_panic]
fn owned_image_rejects_an_ordinary_header_without_actor_facts() {
    const PLAN: ProgramImagePlan = ProgramImagePlan::from_program(SOURCE.eff_list());
    static BYTES: ProgramImageBytes<128> =
        ProgramImageBytes::from_image(SOURCE.eff_list(), PLAN.columns());
    let _ = BYTES.compiled_ref(&SUMMARY, PLAN.columns());
}

#[test]
#[should_panic]
fn owned_image_rejects_a_cyclic_decision_reference() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let left = OWNERSHIP.decisions.offset as usize + 2;
        bytes[left] = 0;
        bytes[left + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_the_reserved_reference() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let receiver = OWNERSHIP.bindings.offset as usize + 2;
        bytes[receiver] = 255;
        bytes[receiver + 1] = 255;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_an_unmapped_choice_scope() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[OWNERSHIP.decisions.offset as usize] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_an_initial_actor_different_from_the_first_sender() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[OWNERSHIP.initial.offset as usize + 1] = 1;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_scope_counts_outside_its_storage() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let header = COLUMNS.ownership_header().offset as usize;
        bytes[header] = 255;
        bytes[header + 1] = 255;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn owned_image_rejects_decision_counts_outside_its_storage() {
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        bytes[COLUMNS.ownership_header().offset as usize + 2] = 2;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}

#[test]
#[should_panic]
fn tagged_ownership_image_rejects_invalid_columns_with_wire_payloads() {
    use crate::global::compiled::images::image::columns::PROGRAM_IMAGE_ATOM_STRIDE;
    static BYTES: [u8; 128] = {
        let mut bytes = encoded_bytes();
        let mut event = 0;
        while event < COLUMNS.atom_count() {
            bytes[event * PROGRAM_IMAGE_ATOM_STRIDE + 7] &= 1;
            event += 1;
        }
        let header = COLUMNS.ownership_header().offset as usize;
        bytes[header + 2] = 255;
        bytes[header + 3] = 255;
        bytes
    };
    let _ = CompiledProgramRef::compact(PROGRAM.facts, COLUMNS, &BYTES);
}
