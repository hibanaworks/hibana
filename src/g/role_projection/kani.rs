//! Query the production cursor's immutable controller facts on real role images.
use super::RoleProjection;
use crate::global::event_program::LocalEventProgram;
use crate::global::{
    compiled::images::RoleDescriptorRef, const_dsl::ScopeId, role_program::RoleImageRef,
};
use crate::{Endpoint, g};

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
    assert!(descriptor.local_len() <= 6);
    // The cursor delegates to this same production query. Mutable progress
    // initialization cannot affect controller membership and is not fabricated
    // as part of this read-only obligation.
    LocalEventProgram::from_rows(rows).contains_route_controller(scope)
}

#[kani::proof]
// These exact images have at most six local rows and one route. The largest
// loop copies a nine-byte atom; ten includes its terminating iteration.
// All scope values remain symbolic and unwinding assertions stay enabled.
#[kani::unwind(10)]
fn initial_resource_issuance_cannot_confer_later_route_control() {
    let raw_scope: u16 = kani::any();
    assert!(!controls(
        &RoleProjection::<2, Steps, 32>::IMAGE_REF,
        raw_scope
    ));
}

#[kani::proof]
#[kani::unwind(10)]
fn received_controller_view_has_only_its_projected_route() {
    let raw_scope: u16 = kani::any();
    assert_eq!(
        controls(&RoleProjection::<1, Steps, 32>::IMAGE_REF, raw_scope),
        raw_scope == ScopeId::route(0).raw()
    );
}

#[kani::proof]
#[kani::unwind(10)]
fn cached_parallel_lane_membership_is_exact_for_every_probe_lane() {
    type Parallel = g::Par<g::Send<0, 1, g::Msg<1, ()>>, g::Send<1, 0, g::Msg<2, ()>>>;
    let rows = &RoleProjection::<0, Parallel, 8>::IMAGE_REF;
    let lane: u16 = kani::any();
    let left = rows.parallel_borrow_mask(0, 0, 1).unwrap();
    let right = rows.parallel_borrow_mask(0, 1, 2).unwrap();
    assert_ne!(left, right);
    assert_eq!(
        rows.parallel_borrow_owns_lane(left, lane as usize),
        lane == 0
    );
    assert_eq!(
        rows.parallel_borrow_owns_lane(right, lane as usize),
        lane == 1
    );
    kani::cover!(lane == 255);
    kani::cover!(lane == u16::MAX);
}
