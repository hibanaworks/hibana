use super::{
    GenTable, LoopDisposition, LoopFrame, LoopTable, PolicyTable, RouteTable, StateSnapshotTable,
};
#[cfg(test)]
mod tests {
    use super::{
        GenTable, LoopDisposition, LoopFrame, LoopTable, PolicyTable, RouteTable,
        StateSnapshotTable,
    };
    use crate::{
        control::types::{Generation, Lane},
        eff::EffIndex,
        global::const_dsl::{ResolverMode, ScopeId},
        transport::FrameLabelMask,
    };
    const ROLE_COUNT: u8 = 2;
    const ROUTE_SLOTS: usize = crate::eff::meta::MAX_EFF_NODES;

    fn allocate_route_storage(route_slots: usize, lane_slots: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(
            RouteTable::storage_bytes(route_slots, lane_slots),
            RouteTable::storage_align(),
        )
        .expect("route table test layout");
        let storage = unsafe { std::alloc::alloc_zeroed(layout) };
        if storage.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        storage
    }

    fn test_route_table(route_slots: usize, lane_base: u32, lane_slots: usize) -> RouteTable {
        let mut table = RouteTable::empty();
        let storage = allocate_route_storage(route_slots, lane_slots);
        unsafe {
            table.bind_from_storage_with_layout(storage, route_slots, lane_base, lane_slots, 0);
        }
        table
    }

    fn tiny_loop_table(loop_slots: usize) -> LoopTable {
        let mut table = LoopTable::empty();
        let frames = std::vec![LoopFrame::free(LoopTable::NO_FRAME); loop_slots].into_boxed_slice();
        let lane_slots = 4usize;
        let lane_heads = std::vec![LoopTable::NO_FRAME; lane_slots].into_boxed_slice();
        let free_head = std::boxed::Box::new(LoopTable::NO_FRAME);
        unsafe {
            table.bind_storage(
                std::boxed::Box::leak(frames).as_mut_ptr(),
                loop_slots,
                0,
                lane_slots,
                std::boxed::Box::leak(lane_heads).as_mut_ptr(),
                std::boxed::Box::leak(free_head),
                0,
            );
        }
        table
    }

    fn tiny_route_table(route_slots: usize) -> RouteTable {
        test_route_table(route_slots, 0, 4)
    }

    fn route_table() -> RouteTable {
        test_route_table(ROUTE_SLOTS, 0, 4)
    }

    fn policy_table(lane_slots: usize) -> PolicyTable {
        let mut table = PolicyTable::empty();
        let bytes = PolicyTable::storage_bytes(lane_slots);
        let mut storage = std::vec![0u8; bytes].into_boxed_slice();
        unsafe {
            table.bind_from_storage(storage.as_mut_ptr(), 0, lane_slots);
        }
        let _ = std::boxed::Box::leak(storage);
        table
    }

    fn gen_table() -> GenTable {
        let mut table = GenTable::empty();
        let bytes = GenTable::storage_bytes(4);
        let mut storage = std::vec![0u8; bytes].into_boxed_slice();
        unsafe {
            table.bind_from_storage(storage.as_mut_ptr(), 0, 4);
        }
        let _ = std::boxed::Box::leak(storage);
        table
    }

    #[test]
    fn gen_table_tracks_presence_with_explicit_mask() {
        let table = gen_table();
        let lane = Lane::new(0);

        assert_eq!(table.last(lane), None);

        table.publish_prepared(lane, Generation::ZERO);
        assert_eq!(table.last(lane), Some(Generation::ZERO));

        table.reset_lane(lane);
        assert_eq!(table.last(lane), None);
        table.publish_prepared(lane, Generation::ZERO);
        assert_eq!(table.last(lane), Some(Generation::ZERO));
    }

    #[test]
    fn gen_table_publish_prepared_records_exact_generation() {
        let table = gen_table();
        let lane = Lane::new(2);

        table.publish_prepared(lane, Generation::ZERO);
        assert_eq!(table.last(lane), Some(Generation::ZERO));
        table.publish_prepared(lane, Generation::new(7));
        assert_eq!(table.last(lane), Some(Generation::new(7)));
        table.publish_prepared(lane, Generation::new(u16::MAX));
        assert_eq!(table.last(lane), Some(Generation::new(u16::MAX)));
    }

    #[test]
    fn gen_table_publish_prepared_rewinds_without_clearing_presence() {
        let table = gen_table();
        let lane = Lane::new(1);

        table.publish_prepared(lane, Generation::ZERO);
        table.publish_prepared(lane, Generation::new(5));
        table.publish_prepared(lane, Generation::new(2));
        assert_eq!(table.last(lane), Some(Generation::new(2)));
        table.publish_prepared(lane, Generation::new(3));
        assert_eq!(table.last(lane), Some(Generation::new(3)));
    }

    #[test]
    fn state_snapshot_table_storage_align_covers_cap_revision() {
        assert!(
            StateSnapshotTable::storage_align() >= core::mem::align_of::<u64>(),
            "snapshot storage must align the cap revision array",
        );
    }

    #[test]
    fn route_table_peek_is_non_consuming() {
        let table = route_table();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        assert_eq!(table.peek_with_role_count(lane, ROLE_COUNT, 1, scope), None);
        table.record_with_role_count(lane, ROLE_COUNT, 0, scope, 1);
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert_eq!(table.peek_with_role_count(lane, ROLE_COUNT, 1, scope), None);
    }

    #[test]
    fn route_table_pending_lane_mask_tracks_unacked_decisions() {
        let table = route_table();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);
        let scope = ScopeId::route(9);

        assert_eq!(
            table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane0),
            false
        );

        table.record_with_role_count(lane0, ROLE_COUNT, 0, scope, 1);
        table.record_with_role_count(lane2, ROLE_COUNT, 0, scope, 1);
        assert_eq!(
            table.has_pending_lane_with_role_count(ROLE_COUNT, 0, scope, lane0),
            false
        );
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane0));
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane2));

        assert_eq!(
            table.acknowledge_with_role_count(lane0, ROLE_COUNT, 1, scope),
            Some(1)
        );
        assert!(!table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane0));
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane2));

        table.record_with_role_count(lane0, ROLE_COUNT, 0, scope, 0);
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane0));
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane2));

        table.reset_lane(lane2);
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane0));
        assert!(!table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope, lane2));
    }

    #[test]
    fn route_table_reuses_lane_slot_after_all_roles_acknowledge() {
        let table = tiny_route_table(1);
        let lane = Lane::new(0);
        let scope_a = ScopeId::route(9);
        let scope_b = ScopeId::route(10);

        table.record_with_role_count(lane, ROLE_COUNT, 0, scope_a, 1);
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope_a, lane));
        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope_a),
            Some(1)
        );
        assert!(!table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope_a, lane));

        table.record_with_role_count(lane, ROLE_COUNT, 0, scope_b, 2);
        assert_eq!(
            table.peek_with_role_count(lane, ROLE_COUNT, 1, scope_b),
            Some(2)
        );
        assert!(table.has_pending_lane_with_role_count(ROLE_COUNT, 1, scope_b, lane));
    }

    #[test]
    fn route_table_change_epoch_tracks_route_and_hint_updates() {
        let table = route_table();
        let lane = Lane::new(0);
        let scope = ScopeId::route(9);

        let initial = table.change_epoch();
        table.record_with_role_count(lane, ROLE_COUNT, 0, scope, 1);
        let after_record = table.change_epoch();
        assert_ne!(after_record, initial);

        assert_eq!(
            table.acknowledge_with_role_count(lane, ROLE_COUNT, 1, scope),
            Some(1)
        );
        let after_ack = table.change_epoch();
        assert_ne!(after_ack, after_record);

        table.update_pending_frame_hint_mask_for_lane(
            lane,
            FrameLabelMask::EMPTY,
            FrameLabelMask::from_frame_label(25),
        );
        let after_hint = table.change_epoch();
        assert_ne!(after_hint, after_ack);

        table.reset_lane(lane);
        assert_ne!(table.change_epoch(), after_hint);
    }

    #[test]
    fn loop_table_reuses_lane_slot_after_lane_reset() {
        let table = tiny_loop_table(1);
        let lane = Lane::new(0);

        assert!(!table.has_decision(lane, 0));
        assert_eq!(table.record(lane, 0, 0, LoopDisposition::Continue), 1);
        assert!(table.has_decision(lane, 0));

        table.reset_lane(lane);
        assert!(!table.has_decision(lane, 0));

        assert_eq!(table.record(lane, 0, 1, LoopDisposition::Break), 1);
        assert!(table.has_decision(lane, 1));
    }

    #[test]
    fn loop_table_supports_distinct_live_lanes_when_budgeted() {
        let table = tiny_loop_table(2);
        let lane0 = Lane::new(0);
        let lane1 = Lane::new(1);

        assert_eq!(table.record(lane0, 0, 0, LoopDisposition::Continue), 1);
        assert_eq!(table.record(lane1, 0, 0, LoopDisposition::Break), 1);

        assert!(table.has_decision(lane0, 0));
        assert!(table.has_decision(lane1, 0));
    }

    #[test]
    fn loop_table_empty_layout_has_no_resident_bytes() {
        assert_eq!(LoopTable::storage_bytes(0, 4), 0);
    }

    #[test]
    fn policy_table_storage_is_sparse_over_lanes() {
        assert_eq!(PolicyTable::storage_bytes(0), 0);
        assert_eq!(
            PolicyTable::storage_bytes(1),
            PolicyTable::storage_bytes(16),
            "policy storage must not multiply by the lane domain"
        );
    }

    #[test]
    fn policy_table_resets_only_matching_lane_entries() {
        let table = policy_table(4);
        let eff = EffIndex::from_dense_ordinal(7);
        let policy_a = ResolverMode::dynamic(1);
        let policy_b = ResolverMode::dynamic(2);

        assert_eq!(table.register(Lane::new(0), eff, 1, policy_a), Ok(()));
        assert_eq!(table.register(Lane::new(2), eff, 1, policy_b), Ok(()));

        assert_eq!(table.get(Lane::new(0), eff, 1), Some(policy_a));
        assert_eq!(table.get(Lane::new(2), eff, 1), Some(policy_b));

        table.reset_lane(Lane::new(0));

        assert_eq!(table.get(Lane::new(0), eff, 1), None);
        assert_eq!(table.get(Lane::new(2), eff, 1), Some(policy_b));
    }

    #[test]
    fn route_table_frame_hint_mask_tracks_buffered_frame_labels() {
        let table = route_table();
        let lane0 = Lane::new(0);
        let lane2 = Lane::new(2);

        assert!(
            !table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(25))
        );

        let frame_labels_25_141 =
            FrameLabelMask::from_frame_label(25) | FrameLabelMask::from_frame_label(141);
        table.update_pending_frame_hint_mask_for_lane(
            lane0,
            FrameLabelMask::EMPTY,
            frame_labels_25_141,
        );
        assert!(table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(25)));
        assert!(table.has_pending_frame_hint_for_lane(lane0, frame_labels_25_141));

        table.update_pending_frame_hint_mask_for_lane(
            lane2,
            FrameLabelMask::EMPTY,
            FrameLabelMask::from_frame_label(141),
        );
        assert!(
            table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(141))
        );
        assert!(
            table.has_pending_frame_hint_for_lane(lane2, FrameLabelMask::from_frame_label(141))
        );

        table.update_pending_frame_hint_mask_for_lane(
            lane0,
            frame_labels_25_141,
            FrameLabelMask::from_frame_label(141),
        );
        assert!(
            !table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(25))
        );
        assert!(
            table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(141))
        );
        assert!(
            table.has_pending_frame_hint_for_lane(lane2, FrameLabelMask::from_frame_label(141))
        );

        table.reset_lane(lane2);
        assert!(
            table.has_pending_frame_hint_for_lane(lane0, FrameLabelMask::from_frame_label(141))
        );
        assert!(
            !table.has_pending_frame_hint_for_lane(lane2, FrameLabelMask::from_frame_label(141))
        );
    }
}
