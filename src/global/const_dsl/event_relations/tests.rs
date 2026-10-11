use super::{RoutePathClasses, events_share_route_path};
use crate::{
    eff::{EffAtom, EventOrigin},
    global::const_dsl::{EffList, ReentryMark, ScopeId, allocation::color_roll_frame_labels},
};

const BYTE_DOMAIN_MASK_BYTES: usize = 32;
const fn insert(mask: &mut [u8; BYTE_DOMAIN_MASK_BYTES], value: u8) {
    mask[(value >> 3) as usize] |= 1u8 << (value & 7);
}
const fn first_available(mask: &[u8; BYTE_DOMAIN_MASK_BYTES]) -> Option<u8> {
    let mut value = 0usize;
    while value < 256 {
        if mask[value >> 3] & (1u8 << (value & 7)) == 0 {
            return Some(value as u8);
        }
        value += 1;
    }
    None
}

// Kept byte-for-byte in control flow from a6339772, independently of the cache.
const fn original_color_roll_frame_labels<const E: usize>(
    eff_list: &mut EffList<E>,
    start: usize,
    end: usize,
) {
    if start >= end || end > eff_list.len() {
        panic!("roll frame-label coloring requires a non-empty body");
    }

    let mut class_idx = start;
    while class_idx < end {
        let class = eff_list.atom_at(class_idx);
        if class.from == class.to {
            class_idx += 1;
            continue;
        }

        let mut already_colored = false;
        let mut prior_idx = start;
        while prior_idx < class_idx {
            let prior = eff_list.atom_at(prior_idx);
            if prior.from == class.from
                && prior.to == class.to
                && prior.lane == class.lane
                && events_share_route_path(eff_list.scope_markers(), prior_idx, class_idx)
            {
                already_colored = true;
                break;
            }
            prior_idx += 1;
        }

        if !already_colored {
            let mut used = [0u8; BYTE_DOMAIN_MASK_BYTES];
            prior_idx = start;
            while prior_idx < class_idx {
                let prior = eff_list.atom_at(prior_idx);
                if prior.from == class.from && prior.to == class.to && prior.lane == class.lane {
                    insert(&mut used, eff_list.frame_label_at(prior_idx));
                }
                prior_idx += 1;
            }
            let Some(color) = first_available(&used) else {
                panic!("roll inbound occurrence coloring exceeds wire domain");
            };

            let mut member_idx = class_idx;
            while member_idx < end {
                let member = eff_list.atom_at(member_idx);
                if member.from == class.from
                    && member.to == class.to
                    && member.lane == class.lane
                    && events_share_route_path(eff_list.scope_markers(), class_idx, member_idx)
                {
                    eff_list.set_frame_label(member_idx, color);
                }
                member_idx += 1;
            }
        }
        class_idx += 1;
    }
}

fn atom(from: u8, to: u8, lane: u8) -> EffAtom {
    EffAtom {
        from,
        to,
        lane,
        label: 0,
        payload_schema: 0,
        origin: EventOrigin::User,
    }
}

fn copy_source<const E: usize>(source: &EffList<E>) -> EffList<E> {
    EffList {
        rows: source.rows,
        scope_marker_start: source.scope_marker_start,
        resolver_start: source.resolver_start,
        source_end: source.source_end,
        len: source.len,
        scope_marker_len: source.scope_marker_len,
        resolver_marker_len: source.resolver_marker_len,
    }
}

fn assert_relation<const E: usize>(source: &EffList<E>, start: usize, end: usize) {
    let classes = RoutePathClasses::<E>::new(source.scope_markers(), start, end);
    for left in start..end {
        for right in start..end {
            assert_eq!(
                classes.share_path(source.scope_markers(), left, right),
                events_share_route_path(source.scope_markers(), left, right),
                "interval {start}..{end}, events {left},{right}"
            );
        }
    }
}

#[test]
fn exact_classes_cover_every_pair_of_small_route_intervals_and_subranges() {
    let mut routes = std::vec::Vec::new();
    for start in 0..5 {
        for split in start + 1..5 {
            for end in split + 1..=5 {
                routes.push((start, split, end));
            }
        }
    }
    for &(a, b, c) in &routes {
        for &(d, e, f) in &routes {
            let mut source = EffList::<13>::new_partitioned(5, 8, 0);
            for _ in 0..5 {
                source.push_event_mut(atom(0, 1, 0));
            }
            source.push_route_scope_mut(ScopeId::route(0), a, b, c, ReentryMark::Reentrant);
            source.push_route_scope_mut(ScopeId::route(1), d, e, f, ReentryMark::SinglePass);
            for start in 0..5 {
                for end in start + 1..=5 {
                    assert_relation(&source, start, end);
                }
            }
        }
    }
}

fn next(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed
}

#[test]
fn cached_coloring_preserves_every_baseline_label_on_generated_sources() {
    for seed in 0..256u32 {
        let mut random = seed;
        let mut source = EffList::<80>::new_partitioned(32, 48, 0);
        for index in 0..32 {
            let from = ((next(&mut random) >> 16) % 4) as u8;
            let to = ((next(&mut random) >> 16) % 4) as u8;
            let lane = ((next(&mut random) >> 16) % 3) as u8;
            source.push_event_mut(atom(from, to, lane));
            source.set_frame_label(index, (next(&mut random) >> 16) as u8);
        }
        for id in 0..12 {
            let start = next(&mut random) as usize % 30;
            let split = start + 1 + next(&mut random) as usize % (31 - start);
            let end = split + 1 + next(&mut random) as usize % (32 - split);
            source.push_route_scope_mut(
                ScopeId::route(id),
                start,
                split,
                end,
                ReentryMark::Reentrant,
            );
        }
        let start = seed as usize % 8;
        let end = 32 - seed as usize % 8;
        assert_relation(&source, start, end);
        let mut baseline = copy_source(&source);
        let mut candidate = source;
        original_color_roll_frame_labels(&mut baseline, start, end);
        color_roll_frame_labels(&mut candidate, start, end);
        for index in 0..32 {
            assert_eq!(
                candidate.frame_label_at(index),
                baseline.frame_label_at(index),
                "seed {seed}, event {index}"
            );
        }
    }
}

fn panic_message(error: std::boxed::Box<dyn core::any::Any + Send>) -> std::string::String {
    if let Some(message) = error.downcast_ref::<&str>() {
        (*message).into()
    } else {
        error.downcast_ref::<std::string::String>().unwrap().clone()
    }
}

#[test]
fn cached_coloring_preserves_wire_exhaustion_and_partial_labels() {
    let mut source = EffList::<774>::new_partitioned(258, 516, 0);
    for _ in 0..258 {
        source.push_event_mut(atom(0, 1, 0));
    }
    for id in 0..129 {
        source.push_route_scope_mut(
            ScopeId::route(id as u16),
            id * 2,
            id * 2 + 1,
            id * 2 + 2,
            ReentryMark::Reentrant,
        );
    }
    let mut baseline = copy_source(&source);
    let mut candidate = source;
    let old = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        original_color_roll_frame_labels(&mut baseline, 0, 258)
    }))
    .unwrap_err();
    let new = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        color_roll_frame_labels(&mut candidate, 0, 258)
    }))
    .unwrap_err();
    assert_eq!(panic_message(new), panic_message(old));
    for index in 0..258 {
        assert_eq!(
            candidate.frame_label_at(index),
            baseline.frame_label_at(index)
        );
    }
}

#[test]
fn compact_and_wide_input_boundaries_preserve_the_exact_relation() {
    let mut source = EffList::<6>::new_partitioned(2, 4, 0);
    source.push_event_mut(atom(0, 1, 0));
    source.push_event_mut(atom(0, 1, 0));
    source.push_route_scope_mut(ScopeId::route(0), 0, 1, 2, ReentryMark::Reentrant);
    let compact = RoutePathClasses::<65536>::new(source.scope_markers(), 0, 65535);
    let wide = RoutePathClasses::<65536>::new(source.scope_markers(), 0, 65536);
    assert!(compact.cached);
    assert!(!wide.cached);
    for left in [0, 1, 2, 65534] {
        for right in [0, 1, 2, 65534] {
            let expected = events_share_route_path(source.scope_markers(), left, right);
            assert_eq!(
                compact.share_path(source.scope_markers(), left, right),
                expected
            );
            assert_eq!(
                wide.share_path(source.scope_markers(), left, right),
                expected
            );
        }
    }
    assert!(wide.share_path(source.scope_markers(), 65534, 65535));
}
