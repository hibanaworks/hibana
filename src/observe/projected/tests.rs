use super::*;
use crate::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};
use futures::executor::block_on;
use std::vec::Vec;

#[test]
fn local_and_alternative_rows_keep_their_distinct_wire_positions() {
    let global = g::seq(
        g::send::<0, 0, g::Msg<99, ()>>(),
        g::route(
            g::send::<0, 1, g::Msg<7, u32>>(),
            g::send::<0, 1, g::Msg<8, u32>>(),
        ),
    );
    let rows: Vec<_> = projected(&project::<0>(&global)).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].id(), ids::PROJECTED_LOCAL);
    assert_eq!(rows[0].arg0(), 99);
    assert_eq!(rows[1].id(), ids::PROJECTED_SEND);
    assert_eq!(rows[1].arg0(), 1 << 16 | 7);
    assert_eq!(rows[2].arg0(), 2 << 16 | 8);
    let mut receiver = projected(&project::<1>(&global));
    assert_eq!(receiver.len(), 2);
    assert_eq!(receiver.next().unwrap().id(), ids::PROJECTED_RECV);
    assert_eq!(receiver.len(), 1);
    assert_eq!(receiver.next().unwrap().ts(), 1);
    assert_eq!(receiver.len(), 0);
    assert!(receiver.next().is_none());
    for (row, event) in rows.iter().enumerate() {
        assert_eq!(event.ts(), row as u32);
        assert_eq!(event.evidence().input()[0], event.arg0());
        assert_eq!(event.evidence().input()[1], event.arg1());
        assert_eq!(event.evidence().input()[2], event.ts());
        assert_eq!(event.evidence().input()[3], u32::from(event.causal_key()));
    }
}

#[test]
fn all_257_wire_labels_match_actual_endpoint_frames_without_truncation() {
    type Message = g::Msg<231, u32>;
    let p = g::send::<0, 1, Message>();
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let p = g::seq(p, p);
    let global = g::seq(p, g::send::<0, 1, Message>());
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let send_rows = projected(&sender_program);
    let recv_rows = projected(&receiver_program);
    assert_eq!(send_rows.len(), 257);
    let transport = TestTransport::new();
    let mut slab = std::vec![0; 128 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(300);
    let mut tap = rv.tap();
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    for (ordinal, (send, recv)) in send_rows.zip(recv_rows).enumerate() {
        assert_eq!(send.id(), ids::PROJECTED_SEND);
        assert_eq!(recv.id(), ids::PROJECTED_RECV);
        assert_eq!((send.arg0(), send.arg1()), (recv.arg0(), recv.arg1()));
        assert_eq!(send.arg0() >> 16, ordinal as u32);
        assert_eq!(send.arg0() as u8, 231);
        block_on(sender.send::<Message>(&(ordinal as u32))).unwrap();
        let (header, _) = transport.queued_frame(1);
        let bytes = header.bytes();
        assert_eq!(u16::from_be_bytes([bytes[7], bytes[8]]), ordinal as u16);
        assert_eq!(bytes[4], (send.arg0() >> 8) as u8);
        assert_eq!(bytes[5], (send.arg1() >> 24) as u8);
        assert_eq!(bytes[6], (send.arg1() >> 16) as u8);
        assert_eq!(
            block_on(receiver.recv::<Message>()).unwrap(),
            ordinal as u32
        );
        let commits: Vec<_> = tap
            .by_ref()
            .filter(|event| matches!(event.id(), ids::ENDPOINT_SEND | ids::ENDPOINT_RECV))
            .collect();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id(), ids::ENDPOINT_SEND);
        assert_eq!(commits[0].causal_key(), 1);
        assert_eq!(commits[1].id(), ids::ENDPOINT_RECV);
        assert_eq!(commits[1].causal_key(), 0x0102);
        for (view, commit) in commits.into_iter().enumerate() {
            assert_eq!(commit.arg0(), sid.raw());
            assert_eq!(commit.arg1() >> 24, view as u32);
            assert_eq!((commit.arg1() >> 16) as u8, (send.arg0() >> 8) as u8);
            assert_eq!(commit.arg1() as u16, ordinal as u16);
        }
    }
    assert!(transport.queue_is_empty());
}

#[test]
fn a_staged_frame_rejected_by_payload_validation_has_no_receive_commit() {
    type Message = g::Msg<231, u32>;
    let global = g::send::<0, 1, Message>();
    let sender_program = project::<0>(&global);
    let receiver_program = project::<1>(&global);
    let transport = TestTransport::new();
    let mut slab = std::vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(302);
    let mut sender = rv.enter(sid, &sender_program).unwrap();
    let mut receiver = rv.enter(sid, &receiver_program).unwrap();
    block_on(sender.send::<Message>(&42)).unwrap();
    let mut tap = rv.tap();
    tap.by_ref().for_each(drop);
    transport.truncate_next_payload(1, 1);
    assert!(block_on(receiver.recv::<Message>()).is_err());
    let events: Vec<_> = tap.collect();
    assert!(
        events
            .iter()
            .any(|event| event.id() == ids::TRANSPORT_FRAME)
    );
    assert!(
        events
            .iter()
            .all(|event| { !matches!(event.id(), ids::ENDPOINT_RECV | ids::ENDPOINT_SESSION) })
    );
}

#[test]
fn delegated_views_and_parallel_lanes_describe_the_frames_they_actually_send() {
    type Loan<'r> = g::Msg<1, Endpoint<'r, 2>>;
    type Read = g::Msg<7, u32>;
    type Value = g::Msg<8, u32>;
    type Tick = g::Msg<7, u32>;
    let global = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::par(g::send::<2, 3, Read>(), g::send::<1, 4, Tick>()),
            g::seq(g::send::<3, 2, Value>(), g::send::<1, 0, Loan>()),
        ),
    );
    let owner_program = project::<0>(&global);
    let worker_program = project::<1>(&global);
    let db_program = project::<2>(&global);
    let service_program = project::<3>(&global);
    let audit_program = project::<4>(&global);
    let owner_rows: Vec<_> = projected(&owner_program).collect();
    let db_rows: Vec<_> = projected(&db_program).collect();
    assert_eq!(owner_rows.len(), db_rows.len());
    for (owner, db) in owner_rows.iter().zip(&db_rows) {
        assert_eq!(owner.causal_key(), 0);
        assert_eq!(db.causal_key(), 2);
        assert_eq!(
            (owner.id(), owner.arg0(), owner.arg1()),
            (db.id(), db.arg0(), db.arg1())
        );
    }
    let worker_rows: Vec<_> = projected(&worker_program).collect();
    assert!(worker_rows.iter().all(|event| event.causal_key() == 0x0101));
    assert!(
        worker_rows
            .iter()
            .any(|event| event.id() == ids::PROJECTED_SEND && event.arg1() >> 24 == 2)
    );
    assert!(
        worker_rows
            .iter()
            .any(|event| event.id() == ids::PROJECTED_RECV && (event.arg1() >> 16) as u8 == 2)
    );
    let transport = TestTransport::new();
    let mut owner_slab = std::vec![0; 64 * 1024];
    let mut worker_slab = std::vec![0; 64 * 1024];
    let mut service_slab = std::vec![0; 64 * 1024];
    let mut audit_slab = std::vec![0; 64 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let mut audit_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, transport.clone())
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, transport.clone())
        .unwrap();
    let service_rv = service_storage
        .init()
        .rendezvous(&mut service_slab, transport.clone())
        .unwrap();
    let audit_rv = audit_storage
        .init()
        .rendezvous(&mut audit_slab, transport.clone())
        .unwrap();
    let sid = SessionId::new(301);
    let mut owner_tap = owner_rv.tap();
    let mut worker_tap = worker_rv.tap();
    let mut service_tap = service_rv.tap();
    let mut audit_tap = audit_rv.tap();
    let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
    let db = owner_rv.enter(sid, &db_program).unwrap();
    let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    let mut audit = audit_rv.enter(sid, &audit_program).unwrap();
    block_on(async {
        owner.send::<Loan>(db).await.unwrap();
        let (_, bytes) = transport.queued_frame(1);
        assert_eq!(bytes[0], (owner_rows[0].arg1() & 0xff) as u8);
        assert_eq!(owner_rows[0].arg1() & 0xffff, 0x0102);
        let mut db = worker.recv::<Loan>().await.unwrap();
        db.send::<Read>(&41).await.unwrap();
        worker.send::<Tick>(&9).await.unwrap();
        for target in [3, 4] {
            let (header, _) = transport.queued_frame(target);
            let bytes = header.bytes();
            let wire = u16::from_be_bytes([bytes[7], bytes[8]]);
            let row = worker_rows
                .iter()
                .find(|row| row.arg0() >> 16 == u32::from(wire))
                .unwrap();
            assert_eq!(row.id(), ids::PROJECTED_SEND);
            assert_eq!((row.arg0() >> 8) as u8, bytes[4]);
            assert_eq!((row.arg1() >> 24) as u8, bytes[5]);
            assert_eq!((row.arg1() >> 16) as u8, bytes[6]);
        }
        let key = service.recv::<Read>().await.unwrap();
        assert_eq!(audit.recv::<Tick>().await.unwrap(), 9);
        service.send::<Value>(&(key + 1)).await.unwrap();
        assert_eq!(db.recv::<Value>().await.unwrap(), 42);
        worker.send::<Loan>(db).await.unwrap();
        drop(owner.recv::<Loan>().await.unwrap());
    });
    assert!(transport.queue_is_empty());
    for (actor, tap, expected) in [
        (0, &mut owner_tap, std::vec![(1, 0, 0), (2, 0, 4)]),
        (
            1,
            &mut worker_tap,
            std::vec![(2, 1, 0), (1, 2, 1), (1, 1, 2), (2, 2, 3), (1, 1, 4)],
        ),
        (3, &mut service_tap, std::vec![(2, 3, 1), (1, 3, 3)]),
        (4, &mut audit_tap, std::vec![(2, 4, 2)]),
    ] {
        let commits: Vec<_> = tap
            .filter(|event| matches!(event.id(), ids::ENDPOINT_SEND | ids::ENDPOINT_RECV))
            .map(|event| {
                assert_eq!(event.causal_key() >> 8, actor);
                assert_eq!(event.arg0(), sid.raw());
                (
                    event.causal_key() as u8,
                    (event.arg1() >> 24) as u8,
                    event.arg1() as u16,
                )
            })
            .collect();
        assert_eq!(commits, expected);
    }
}
