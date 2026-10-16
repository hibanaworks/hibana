//! Query production cursor navigation over the actual owned role images.
use super::RoleProjection;
use crate::global::{
    compiled::images::RoleDescriptorRef,
    const_dsl::ScopeId,
    role_program::RoleImageRef,
    typestate::{EventCursor, EventCursorState},
};
use crate::{Endpoint, g};
use core::mem::MaybeUninit;

type Loan = g::Msg<1, Endpoint<'static, 10>>;
type Read = g::Msg<2, u32>;
type Subtract = g::Msg<3, u32>;
type Value = g::Msg<4, u32>;
type Steps = g::Seq<
    g::Send<2, 0, Loan>,
    g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Resolve<
                g::Route<
                    g::Seq<g::Send<10, 2, Read>, g::Send<2, 10, Value>>,
                    g::Seq<g::Send<10, 2, Subtract>, g::Send<2, 10, Value>>,
                >,
                702,
            >,
            g::Send<1, 0, Loan>,
        >,
    >,
>;

fn controls(rows: &'static RoleImageRef, raw_scope: u16) -> bool {
    let Some(scope) = ScopeId::decode_raw(raw_scope) else {
        return false;
    };
    let descriptor = RoleDescriptorRef::from_resident(rows);
    assert_eq!(descriptor.logical_lane_count(), 1);
    assert!(descriptor.local_len() <= 32);
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0; 1];
    let mut labels = [0; 1];
    let mut completed = [0; 1];
    // SAFETY: disjoint backing columns stay stationary through the query and
    // have the descriptor's exact lane domain and sufficient completion bits.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
        cursor.assume_init_ref().contains_route_controller(scope)
    }
}

#[kani::proof]
#[kani::unwind(32)]
fn initial_resource_issuance_cannot_confer_later_route_control() {
    let raw_scope: u16 = kani::any();
    assert!(!controls(
        &RoleProjection::<2, Steps, 32>::IMAGE_REF,
        raw_scope
    ));
}

#[kani::proof]
#[kani::unwind(32)]
fn received_controller_view_has_only_its_projected_route() {
    let raw_scope: u16 = kani::any();
    assert_eq!(
        controls(&RoleProjection::<1, Steps, 32>::IMAGE_REF, raw_scope),
        raw_scope == ScopeId::route(0).raw()
    );
}
