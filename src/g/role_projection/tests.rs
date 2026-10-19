//! Drive the actual compact cursor built by actor-aware production lowering.
//! Cursor and actual owned Endpoint execution over private actor images.
//! Full semantic acceptance and distributed guarantees remain unfinished;
//! public owned projection remains rejected.
use super::RoleProjection;
use crate::global::{
    compiled::images::RoleDescriptorRef,
    role_program::RoleImageRef,
    typestate::{EventCommitMeta, EventCursor, EventCursorState, LocalAction},
};
use crate::{Endpoint, g};
use core::mem::MaybeUninit;
mod admission;
mod carrier_contract;
mod commit;
mod composed;
mod conditional_controller;
mod controller_columns;
mod distributed_source;
mod entry_image;
mod fifo_origin;
#[cfg(not(miri))]
mod frame_domains;
mod handoff_interval;
mod ingress;
mod ingress_columns;
mod local_return;
mod logical_view_word;
mod mixed_send_admission;
mod multi_owner_roll;
mod mutation_coverage;
#[cfg(all(unix, not(miri)))]
mod native;
mod occurrence_choice;
mod order;
mod owned;
mod parallel_borrow;
mod parallel_pending;
mod partial_choice;
mod pending_reentry;
mod program_columns;
mod receipt_alternatives;
mod receipt_refresh;
mod recovery_image;
mod reentry;
mod reentry_choice;
mod reentry_columns;
#[cfg(not(miri))]
mod reentry_commit;
mod reentry_parallel;
mod retirement;
mod return_reentry;
mod roll_exit;
mod routing;
mod send_wait;
mod source_order;
mod transitive_borrow;
mod visit_progress;

#[test]
fn initial_resource_views_share_progress_and_park_independent_receives() {
    use crate::{
        runtime::{SessionKitStorage, ids::SessionId},
        test_support::transport::TestTransport,
    };
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    type Steps = g::Seq<g::Par<g::Send<2, 0, Read>, g::Send<3, 10, Read>>, g::Send<0, 1, Loan>>;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 8>::IMAGE_REF,
    );
    let left_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 8>::IMAGE_REF,
    );
    let right_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 8>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 32 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        let sid = SessionId::new(820);
        // Either logical view may attach first. Both must retain the same
        // pinned actor, without creating another cursor or another issuer.
        let mut db = rv.enter(sid, &resource_program).unwrap();
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        assert!(rv.enter(sid, &resource_program).is_err());
        assert!(rv.enter(sid, &owner_program).is_err());
        let mut left = rv.enter(sid, &left_program).unwrap();
        let mut right = rv.enter(sid, &right_program).unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut owner_recv = pin!(owner.recv::<Read>());
            let mut db_recv = pin!(db.recv::<Read>());
            assert!(owner_recv.as_mut().poll(&mut cx).is_pending());
            let db_poll = db_recv.as_mut().poll(&mut cx);
            assert!(db_poll.is_pending(), "{db_poll:?}");
            right.send::<Read>(&19).await.unwrap();
            assert!(matches!(
                db_recv.as_mut().poll(&mut cx),
                Poll::Ready(Ok(19))
            ));
            assert!(owner_recv.as_mut().poll(&mut cx).is_pending());
            left.send::<Read>(&7).await.unwrap();
            assert!(matches!(
                owner_recv.as_mut().poll(&mut cx),
                Poll::Ready(Ok(7))
            ));
        }
        // The pending Loan still requires the unique DB view. Dropping it
        // cannot reset or reissue that view through enter.
        drop(db);
        assert!(rv.enter(sid, &resource_program).is_err());
    });
}

type Loan = g::Msg<1, Endpoint<'static, 10>>;
type OtherLoan = g::Msg<2, Endpoint<'static, 11>>;
type Read = g::Msg<3, u32>;
type Value = g::Msg<4, u32>;
type DbLoan = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<g::Send<10, 3, Read>, g::Seq<g::Send<3, 10, Value>, g::Send<1, 0, Loan>>>,
>;
type AuthLoan = g::Seq<
    g::Send<0, 1, OtherLoan>,
    g::Seq<g::Send<11, 4, Read>, g::Seq<g::Send<4, 11, Value>, g::Send<1, 0, OtherLoan>>>,
>;

#[test]
fn owned_send_recv_returns_the_resource_continuation_to_its_original_actor() {
    use crate::{
        runtime::{SessionKitStorage, ids::SessionId},
        test_support::transport::TestTransport,
    };
    type Steps = g::Seq<DbLoan, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut slab = [0u8; 64 * 1024];
        let mut storage = SessionKitStorage::<TestTransport>::uninit();
        let kit = storage.init();
        let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
        let sid = SessionId::new(821);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = rv.enter(sid, &worker_program).unwrap();
        let mut service = rv.enter(sid, &service_program).unwrap();
        let (returned, (), ()) = tokio::join!(
            async {
                owner.send::<Loan>(db).await.unwrap();
                let mut db = owner.recv::<Loan>().await.unwrap();
                // Holding the received DB does not borrow owner or its frame.
                drop(owner);
                assert!(rv.enter(sid, &resource_program).is_err());
                db.send::<Read>(&9).await.unwrap();
                db.recv::<Value>().await.unwrap()
            },
            async {
                let mut db = worker.recv::<Loan>().await.unwrap();
                db.send::<Read>(&7).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                worker.send::<Loan>(db).await.unwrap();
            },
            async {
                let key = service.recv::<Read>().await.unwrap();
                assert_eq!(key, 7);
                service.send::<Value>(&(key + 1)).await.unwrap();
                let key = service.recv::<Read>().await.unwrap();
                assert_eq!(key, 9);
                service.send::<Value>(&(key + 1)).await.unwrap();
            }
        );
        assert_eq!(returned, 10);
    });
}

#[test]
fn owned_send_recv_crosses_distinct_runtime_slabs_and_returns_the_continuation() {
    use crate::{
        runtime::{SessionKitStorage, ids::SessionId},
        test_support::transport::TestTransport,
    };
    type Steps = g::Seq<DbLoan, g::Seq<g::Send<10, 3, Read>, g::Send<3, 10, Value>>>;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 32>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 32>::IMAGE_REF,
    );
    let resource_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Steps, 32>::IMAGE_REF,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut owner_slab = [0u8; 32 * 1024];
        let mut worker_slab = [0u8; 32 * 1024];
        let mut service_slab = [0u8; 32 * 1024];
        let mut owner_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut worker_storage = SessionKitStorage::<TestTransport>::uninit();
        let mut service_storage = SessionKitStorage::<TestTransport>::uninit();
        let network = TestTransport::new();
        let owner_kit = owner_storage.init();
        let worker_kit = worker_storage.init();
        let service_kit = service_storage.init();
        let rv = owner_kit
            .rendezvous(&mut owner_slab, network.clone())
            .unwrap();
        let worker_rv = worker_kit
            .rendezvous(&mut worker_slab, network.clone())
            .unwrap();
        let service_rv = service_kit.rendezvous(&mut service_slab, network).unwrap();
        let sid = SessionId::new(821);
        let mut owner = rv.enter(sid, &owner_program).unwrap();
        let db = rv.enter(sid, &resource_program).unwrap();
        let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
        let mut service = service_rv.enter(sid, &service_program).unwrap();
        let (returned, (), ()) = tokio::join!(
            async {
                owner.send::<Loan>(db).await.unwrap();
                let mut db = owner.recv::<Loan>().await.unwrap();
                // Holding the received DB does not borrow owner or its frame.
                drop(owner);
                assert!(rv.enter(sid, &resource_program).is_err());
                db.send::<Read>(&9).await.unwrap();
                db.recv::<Value>().await.unwrap()
            },
            async {
                let mut db = worker.recv::<Loan>().await.unwrap();
                db.send::<Read>(&7).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 8);
                worker.send::<Loan>(db).await.unwrap();
            },
            async {
                let key = service.recv::<Read>().await.unwrap();
                assert_eq!(key, 7);
                service.send::<Value>(&(key + 1)).await.unwrap();
                let key = service.recv::<Read>().await.unwrap();
                assert_eq!(key, 9);
                service.send::<Value>(&(key + 1)).await.unwrap();
            }
        );
        assert_eq!(returned, 10);
    });
}

fn with_cursor(image: &'static RoleImageRef, run: impl FnOnce(&mut EventCursor)) {
    let descriptor = RoleDescriptorRef::from_resident(image);
    assert!(image.local_step_count() <= 32);
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 256];
    let mut labels = [0u16; 256];
    let mut completed = [0u32; 1];
    // SAFETY: all disjoint backing arrays remain stationary for the callback;
    // no cursor, state reference or borrowed backing pointer escapes it.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
        run(cursor.assume_init_mut());
    }
}

fn event(image: &RoleImageRef, step: usize) -> EventCommitMeta {
    let node = image.local_step_node(step).unwrap();
    let (eff_index, label, origin, lane) = match node.action() {
        LocalAction::Send {
            eff_index,
            label,
            origin,
            lane,
            ..
        }
        | LocalAction::Recv {
            eff_index,
            label,
            origin,
            lane,
            ..
        }
        | LocalAction::Local {
            eff_index,
            label,
            origin,
            lane,
            ..
        } => (eff_index, label, origin, lane),
        LocalAction::Terminate => panic!("expected an operation"),
    };
    EventCommitMeta::new(
        eff_index,
        label,
        origin,
        node.scope(),
        node.route_arm(),
        lane,
    )
}

#[test]
fn later_conditional_bindings_keep_their_original_route_context() {
    type Left = g::Seq<g::Send<0, 1, Loan>, g::Send<0, 2, g::Msg<5, ()>>>;
    type Right = g::Seq<g::Send<0, 1, g::Msg<6, ()>>, g::Send<0, 2, Loan>>;
    type Steps = g::Seq<g::Resolve<g::Route<Left, Right>, 701>, g::Send<10, 3, Read>>;
    let holder = &RoleProjection::<1, Steps, 32>::IMAGE_REF;
    let peer = &RoleProjection::<3, Steps, 32>::IMAGE_REF;
    let absent = &RoleProjection::<4, Steps, 32>::IMAGE_REF;
    assert_eq!(holder.local_step_count(), 3);
    assert_eq!(peer.local_step_count(), 1);
    assert_eq!(peer.footprint().route_scope_count, 1);
    let route = peer.route_scope_by_slot(0).unwrap();
    assert_eq!(route, holder.route_scope_by_slot(0).unwrap());
    assert!(peer.local_step_node(0).unwrap().scope().is_none());
    assert_eq!(peer.program().actor_binding_at(4), Some([256, 3]));
    for arm in 0..2 {
        assert_eq!(peer.route_arm_event_row_by_slot(0, arm).len(), 0);
    }
    // Retention is determined by used binding references. A role with neither
    // operations nor a conditional peer gets no unrelated context rows.
    assert_eq!(absent.local_step_count(), 0);
    assert_eq!(absent.footprint().route_scope_count, 0);
    // The candidate data deliberately leaves the binding conditional. This is
    // not proof of context acquisition or permission to execute either path.
    assert_eq!(holder.program().actor_binding_at(4), Some([256, 3]));
}

#[test]
fn resource_and_holder_operations_share_one_cursor_and_reject_early_return() {
    let image = &RoleProjection::<1, DbLoan, 32>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 4);
    assert!(image.local_step_node(1).unwrap().action().is_send());
    assert!(image.local_step_node(2).unwrap().action().is_recv());
    with_cursor(image, |cursor| {
        for step in 0..4 {
            if step != 3 {
                assert!(
                    cursor
                        .event_enabled(3, event(image, 3), &mut |_, _| None)
                        .is_err()
                );
            }
            let commit = cursor
                .event_enabled(step, event(image, step), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
            cursor.set_index(commit.cursor_after().as_usize());
        }
        assert!(cursor.is_complete(&mut |_| None));
    });
    assert_eq!(
        RoleProjection::<0, DbLoan, 32>::IMAGE_REF.local_step_count(),
        2
    );
    assert_eq!(
        RoleProjection::<10, DbLoan, 32>::IMAGE_REF.local_step_count(),
        2
    );
}

#[test]
fn distinct_logical_facets_at_one_actor_are_two_cursor_steps() {
    type Steps = g::Seq<g::Send<0, 1, Loan>, g::Send<10, 1, Read>>;
    let image = &RoleProjection::<1, Steps, 8>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 3);
    assert!(image.local_step_node(1).unwrap().action().is_send());
    assert!(image.local_step_node(2).unwrap().action().is_recv());
    with_cursor(image, |cursor| {
        for step in 0..3 {
            if step < 2 {
                assert!(
                    cursor
                        .event_enabled(2, event(image, 2), &mut |_, _| None)
                        .is_err()
                );
            }
            let commit = cursor
                .event_enabled(step, event(image, step), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
            cursor.set_index(commit.cursor_after().as_usize());
        }
        assert!(cursor.is_complete(&mut |_| None));
    });
}

#[test]
fn shared_holder_can_finish_either_parallel_loan_first_and_join_requires_both() {
    type Steps = g::Seq<g::Par<g::Roll<DbLoan>, g::Roll<AuthLoan>>, g::Send<1, 0, g::Msg<9, ()>>>;
    let image = &RoleProjection::<1, Steps, 32>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 9);
    assert_eq!(image.footprint().active_lane_count, 2);
    for (slot, start) in [(0, 0), (1, 4)] {
        let range = image.roll_scope_row(slot).unwrap().event_row();
        assert_eq!((range.start(), range.end()), (start, start + 4));
        assert!(image.dependency_for_index(start).is_none());
    }
    let join = image.dependency_for_index(8).unwrap();
    assert_eq!((join.start(), join.end()), (0, 8));
    for order in [
        [0, 1, 2, 3, 4, 5, 6, 7, 8],
        [4, 5, 6, 7, 0, 1, 2, 3, 8],
        [0, 4, 5, 1, 6, 2, 7, 3, 8],
    ] {
        with_cursor(image, |cursor| {
            assert!(
                cursor
                    .event_enabled(0, event(image, 0), &mut |_, _| None)
                    .is_ok()
            );
            assert!(
                cursor
                    .event_enabled(4, event(image, 4), &mut |_, _| None)
                    .is_ok()
            );
            for step in order {
                if step != 8 {
                    assert!(
                        cursor
                            .event_enabled(8, event(image, 8), &mut |_, _| None)
                            .is_err()
                    );
                }
                let commit = cursor
                    .event_enabled(step, event(image, step), &mut |_, _| None)
                    .unwrap();
                cursor.advance_lane_to_relocatable_step(commit.progress_step());
                cursor.set_index(commit.cursor_after().as_usize());
            }
            assert!(cursor.is_complete(&mut |_| None));
        });
    }
}

#[test]
fn resolved_route_expands_both_facets_in_the_original_arm_ranges() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Roll<
            g::Resolve<
                g::Route<g::Send<10, 1, g::Msg<5, u32>>, g::Send<10, 1, g::Msg<6, u32>>>,
                701,
            >,
        >,
    >;
    let image = &RoleProjection::<1, Steps, 32>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 5);
    assert_eq!(image.footprint().route_scope_count, 1);
    for (arm, start) in [(0, 1), (1, 3)] {
        let range = image.route_arm_event_row_by_slot(0, arm);
        assert_eq!((range.start(), range.end()), (start, start + 2));
        assert!(image.local_step_node(start).unwrap().action().is_send());
        assert!(image.local_step_node(start + 1).unwrap().action().is_recv());
    }
    let range = image.roll_scope_row(0).unwrap().event_row();
    assert_eq!((range.start(), range.end()), (1, 5));
}
