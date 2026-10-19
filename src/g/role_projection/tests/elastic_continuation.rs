use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

type Start = g::Msg<201, u32>;
type Ack = g::Msg<202, u32>;
type Finish = g::Msg<203, u32>;
type Body = g::Seq<g::Send<0, 1, Start>, g::Send<1, 0, Ack>>;
type Steps = g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>;

#[test]
fn a_queued_old_continuation_survives_a_new_body_visit() {
    let () = g::ProgramProjection::<Steps, 8>::VALIDATION;
    let a_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 8>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 8>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut a_slab = [0; 16 * 1024];
            let mut b_slab = [0; 16 * 1024];
            let mut a_storage = SessionKitStorage::uninit();
            let mut b_storage = SessionKitStorage::uninit();
            let a_rv = a_storage
                .init()
                .rendezvous(&mut a_slab, network.clone())
                .unwrap();
            let b_rv = b_storage
                .init()
                .rendezvous(&mut b_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9072);
            let mut a = a_rv.enter(sid, &a_program).unwrap();
            let mut b = b_rv.enter(sid, &b_program).unwrap();
            tokio::time::timeout(core::time::Duration::from_millis(100), async {
                a.send::<Start>(&7).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                b.send::<Ack>(&8).await.unwrap();
                assert_eq!(a.recv::<Ack>().await.unwrap(), 8);
                b.send::<Finish>(&9).await.unwrap();
                a.send::<Start>(&10).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                b.send::<Ack>(&11).await.unwrap();
                assert_eq!(a.recv::<Finish>().await.unwrap(), 9);
                assert_eq!(a.recv::<Ack>().await.unwrap(), 11);
                a.send::<Start>(&12).await.unwrap();
                assert_eq!(b.recv::<Start>().await.unwrap(), 12);
                b.send::<Ack>(&13).await.unwrap();
                assert_eq!(a.recv::<Ack>().await.unwrap(), 13);
            })
            .await
            .expect("an accepted old continuation must not strand the new visit");
            assert!(network.queue_is_empty());
        });
}

#[test]
fn a_queued_old_branch_does_not_skip_the_fresh_body_receive() {
    type Stop = g::Msg<204, u32>;
    type OtherAck = g::Msg<208, u32>;
    type Final = g::Msg<209, u32>;
    type ChoiceBody =
        g::Seq<g::Send<0, 1, Start>, g::Route<g::Send<1, 0, Ack>, g::Send<1, 0, OtherAck>>>;
    type Choice = g::Seq<
        g::Roll<ChoiceBody>,
        g::Seq<g::Route<g::Send<1, 0, Finish>, g::Send<1, 0, Stop>>, g::Send<0, 1, Final>>,
    >;
    let () = g::ProgramProjection::<Choice, 32>::VALIDATION;
    let a_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Choice, 32>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Choice, 32>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for expected_label in [203, 204] {
                let network = TestTransport::new();
                let mut a_slab = [0; 16 * 1024];
                let mut b_slab = [0; 16 * 1024];
                let mut a_storage = SessionKitStorage::uninit();
                let mut b_storage = SessionKitStorage::uninit();
                let a_rv = a_storage
                    .init()
                    .rendezvous(&mut a_slab, network.clone())
                    .unwrap();
                let b_rv = b_storage
                    .init()
                    .rendezvous(&mut b_slab, network.clone())
                    .unwrap();
                let sid = SessionId::new(9073);
                let mut a = a_rv.enter(sid, &a_program).unwrap();
                let mut b = b_rv.enter(sid, &b_program).unwrap();
                tokio::time::timeout(core::time::Duration::from_millis(100), async {
                    a.send::<Start>(&7).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                    b.send::<Ack>(&8).await.unwrap();
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), 202);
                    assert_eq!(branch.recv::<Ack>().await.unwrap(), 8);
                    match expected_label {
                        203 => b.send::<Finish>(&9).await.unwrap(),
                        204 => b.send::<Stop>(&9).await.unwrap(),
                        _ => unreachable!(),
                    }
                    a.send::<Start>(&10).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                    b.send::<OtherAck>(&11).await.unwrap();
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), expected_label);
                    match branch.label() {
                        203 => assert_eq!(branch.recv::<Finish>().await.unwrap(), 9),
                        204 => assert_eq!(branch.recv::<Stop>().await.unwrap(), 9),
                        _ => unreachable!(),
                    }
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), 208);
                    assert_eq!(branch.recv::<OtherAck>().await.unwrap(), 11);
                    a.send::<Final>(&14).await.unwrap();
                    assert_eq!(b.recv::<Final>().await.unwrap(), 14);
                })
                .await
                .expect("an earlier branch cannot hide the fresh body head");
                assert!(network.queue_is_empty());
            }
        });
}

#[test]
fn retained_completion_never_supplies_a_fresh_body_predecessor() {
    let image = &RoleProjection::<0, Steps, 8>::IMAGE_REF;
    let roll = image.roll_scope_row(0).unwrap().scope();
    with_cursor(image, |cursor| {
        for index in 0..2 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        cursor.clear_reentry_scope_events(roll);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_ok()
        );
        assert!(
            cursor
                .event_enabled(1, event(image, 1), &mut |_, _| None)
                .is_err()
        );
    });
}

#[test]
fn restarting_an_outer_visit_discards_its_old_inner_completion() {
    type Nested = g::Roll<g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>>;
    let () = g::ProgramProjection::<Nested, 8>::VALIDATION;
    let image = &RoleProjection::<0, Nested, 8>::IMAGE_REF;
    let scopes = [
        image.roll_scope_row(0).unwrap(),
        image.roll_scope_row(1).unwrap(),
    ];
    let inner = scopes
        .iter()
        .find(|row| row.event_row().len() == 2)
        .unwrap()
        .scope();
    let outer = scopes
        .iter()
        .find(|row| row.event_row().len() == 3)
        .unwrap()
        .scope();
    with_cursor(image, |cursor| {
        for index in 0..2 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        cursor.clear_reentry_scope_events(inner);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_ok()
        );
        for index in 0..3 {
            let commit = cursor
                .event_enabled(index, event(image, index), &mut |_, _| None)
                .unwrap();
            cursor.advance_lane_to_relocatable_step(commit.progress_step());
        }
        cursor.clear_reentry_scope_events(outer);
        assert!(
            cursor
                .event_enabled(2, event(image, 2), &mut |_, _| None)
                .is_err()
        );
    });
}

#[test]
fn retained_roll_completion_has_storage_past_a_full_event_word() {
    type Tail = g::Send<0, 1, g::Msg<205, ()>>;
    type Two = g::Seq<Tail, Tail>;
    type Four = g::Seq<Two, Two>;
    type Eight = g::Seq<Four, Four>;
    type Sixteen = g::Seq<Eight, Eight>;
    type Boundary = g::Seq<
        g::Roll<g::Send<0, 1, g::Msg<206, ()>>>,
        g::Seq<Sixteen, g::Seq<Eight, g::Seq<Four, g::Seq<Two, Tail>>>>,
    >;
    let () = g::ProgramProjection::<Boundary, 128>::VALIDATION;
    let image = &RoleProjection::<0, Boundary, 128>::IMAGE_REF;
    assert_eq!(image.local_step_count(), 32);
    assert_eq!(image.footprint().completion_bit_count(), 33);
    with_cursor(image, |cursor| {
        let commit = cursor
            .event_enabled(0, event(image, 0), &mut |_, _| None)
            .unwrap();
        cursor.advance_lane_to_relocatable_step(commit.progress_step());
        cursor.clear_reentry_scope_events(image.roll_scope_row(0).unwrap().scope());
        assert!(cursor.has_retained_roll_completion());
        for index in 0..32 {
            let lane = image.local_step_lane(index).unwrap();
            assert!(!cursor.node_event_done_for_lane(index, lane));
        }
        assert!(
            cursor
                .event_enabled(1, event(image, 1), &mut |_, _| None)
                .is_err(),
            "a retained visit cannot authorize a new outgoing acceptance"
        );
    });
}

#[test]
fn a_late_branch_is_available_while_the_new_body_head_is_ordinary() {
    type Stop = g::Msg<204, u32>;
    type Choice = g::Seq<g::Roll<Body>, g::Route<g::Send<1, 0, Finish>, g::Send<1, 0, Stop>>>;
    let () = g::ProgramProjection::<Choice, 16>::VALIDATION;
    let a_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Choice, 16>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Choice, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            for expected_label in [203, 204] {
                let network = TestTransport::new();
                let mut a_slab = [0; 16 * 1024];
                let mut b_slab = [0; 16 * 1024];
                let mut a_storage = SessionKitStorage::uninit();
                let mut b_storage = SessionKitStorage::uninit();
                let a_rv = a_storage
                    .init()
                    .rendezvous(&mut a_slab, network.clone())
                    .unwrap();
                let b_rv = b_storage
                    .init()
                    .rendezvous(&mut b_slab, network.clone())
                    .unwrap();
                let sid = SessionId::new(9074);
                let mut a = a_rv.enter(sid, &a_program).unwrap();
                let mut b = b_rv.enter(sid, &b_program).unwrap();
                tokio::time::timeout(core::time::Duration::from_millis(100), async {
                    a.send::<Start>(&7).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 7);
                    b.send::<Ack>(&8).await.unwrap();
                    assert_eq!(a.recv::<Ack>().await.unwrap(), 8);
                    match expected_label {
                        203 => b.send::<Finish>(&9).await.unwrap(),
                        204 => b.send::<Stop>(&9).await.unwrap(),
                        _ => unreachable!(),
                    }
                    a.send::<Start>(&10).await.unwrap();
                    assert_eq!(b.recv::<Start>().await.unwrap(), 10);
                    b.send::<Ack>(&11).await.unwrap();
                    let branch = a.offer().await.unwrap();
                    assert_eq!(branch.label(), expected_label);
                    match branch.label() {
                        203 => assert_eq!(branch.recv::<Finish>().await.unwrap(), 9),
                        204 => assert_eq!(branch.recv::<Stop>().await.unwrap(), 9),
                        _ => unreachable!(),
                    }
                    assert_eq!(a.recv::<Ack>().await.unwrap(), 11);
                })
                .await
                .expect("a late branch remains observable beside an ordinary body head");
                assert!(network.queue_is_empty());
            }
        });
}
