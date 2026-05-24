// Offer-path kernel regression tests split by behavior owner.
include!("compact_and_helpers.rs");
include!("fixture_storage.rs");
include!("hint_route_fixture.rs");
include!("attach_and_basic_offer.rs");
include!("frontier_observation.rs");
include!("controller_binding_masks.rs");
include!("binding_frame_mismatch.rs");
include!("rollback_and_nested_offer.rs");
include!("static_linger_routes.rs");
include!("hint_conflict_recovery.rs");
include!("iterative_route_replies.rs");
include!("passive_hint_and_terminal_faults.rs");
