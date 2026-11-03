//! Actual resource loans return only after their current I/O completions.
mod common;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use hibana::{
    Endpoint, EndpointError,
    g::{self, Msg},
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{Projectable, RoleProgram, project},
    },
};

const RX: u8 = 0;
const TX: u8 = 1;
const OWNER: u8 = 2;
const NEXT: u8 = 3;
const R: u8 = 4;
const S: u8 = 5;

type LoanR<'r> = Msg<9, Endpoint<'r, R>>;
type LoanS<'r> = Msg<10, Endpoint<'r, S>>;
type Read = Msg<0, u64>;
type ReadOk = Msg<1, u64>;
type ReadFailed = Msg<2, u64>;
type Write = Msg<3, u64>;
type WriteOk = Msg<4, u64>;
type WriteFailed = Msg<5, u64>;
type Reuse = Msg<6, u64>;
type Abort = Msg<7, u64>;
type Taken = Msg<8, u64>;
type ReusedR = Msg<11, u64>;
type ReusedS = Msg<12, u64>;

fn choreography() -> impl Projectable {
    g::seq(
        g::par(
            g::seq(
                g::send::<OWNER, RX, LoanR>(),
                g::seq(
                    g::send::<OWNER, R, Read>(),
                    g::seq(
                        g::route(
                            g::send::<R, OWNER, ReadOk>(),
                            g::send::<R, OWNER, ReadFailed>(),
                        ),
                        g::send::<RX, OWNER, LoanR>(),
                    ),
                ),
            ),
            g::seq(
                g::send::<OWNER, TX, LoanS>(),
                g::seq(
                    g::send::<OWNER, S, Write>(),
                    g::seq(
                        g::route(
                            g::send::<S, OWNER, WriteOk>(),
                            g::send::<S, OWNER, WriteFailed>(),
                        ),
                        g::send::<TX, OWNER, LoanS>(),
                    ),
                ),
            ),
        ),
        g::route(
            g::seq(
                g::send::<OWNER, NEXT, Reuse>(),
                g::seq(
                    g::send::<OWNER, NEXT, LoanR>(),
                    g::seq(
                        g::send::<OWNER, NEXT, LoanS>(),
                        g::seq(
                            g::par(
                                g::send::<R, OWNER, ReusedR>(),
                                g::send::<S, OWNER, ReusedS>(),
                            ),
                            g::send::<NEXT, OWNER, Taken>(),
                        ),
                    ),
                ),
            ),
            g::send::<OWNER, NEXT, Abort>(),
        ),
    )
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn normal_reuse_requires_both_current_successful_completions() -> Result<(), EndpointError> {
    let global = choreography();
    let (p0, p1, p2, p3, p4, p5) = (
        project::<RX>(&global),
        project::<TX>(&global),
        project::<OWNER>(&global),
        project::<NEXT>(&global),
        project::<R>(&global),
        project::<S>(&global),
    );
    for (rx_ok, tx_ok, stale) in [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let network = TestTransport::new();
        let [mut slab0, mut slab1, mut slab2, mut slab3] = [[0; 32 * 1024]; 4];
        let [mut s0, mut s1, mut s2, mut s3] = [(); 4].map(|_| SessionKitStorage::uninit());
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
        let rv3 = s3.init().rendezvous(&mut slab3, network.clone()).unwrap();
        let sid = SessionId::new(0x7070);
        let mut owner = rv0.enter(sid, &p2).unwrap();
        let r = rv0.enter(sid, &p4).unwrap();
        let s = rv0.enter(sid, &p5).unwrap();
        let mut rx = rv1.enter(sid, &p0).unwrap();
        let mut tx = rv2.enter(sid, &p1).unwrap();
        let mut next = rv3.enter(sid, &p3).unwrap();
        let mut read_bytes = Vec::new();
        let mut written_bytes = Vec::new();
        async {
            async {
                let ((r, read), (s, written)) = async {
                    let [mut read, mut write] = owner.par()?;
                    async {
                        read.send::<LoanR>(r).await?;
                        read.send::<Read>(&7).await?;
                        let branch = read.offer().await?;
                        let result = match branch.label() {
                            1 => Some(branch.recv::<ReadOk>().await?),
                            2 => {
                                branch.recv::<ReadFailed>().await?;
                                None
                            }
                            label => panic!("unexpected read result {label}"),
                        };
                        let r = read.recv::<LoanR>().await?;
                        Ok((r, result))
                    };
                    async {
                        write.send::<LoanS>(s).await?;
                        write.send::<Write>(&7).await?;
                        let branch = write.offer().await?;
                        let result = match branch.label() {
                            4 => Some(branch.recv::<WriteOk>().await?),
                            5 => {
                                branch.recv::<WriteFailed>().await?;
                                None
                            }
                            label => panic!("unexpected write result {label}"),
                        };
                        let s = write.recv::<LoanS>().await?;
                        Ok((s, result))
                    };
                }
                .await?;
                // Result values are application data. Affine custody and the
                // communication join are enforced by the actual Endpoint loans.
                if read == Some(7) && written == Some(7) {
                    owner.send::<Reuse>(&7).await?;
                    owner.send::<LoanR>(r).await?;
                    owner.send::<LoanS>(s).await?;
                    let (read, written) = async {
                        let [mut read, mut write] = owner.par()?;
                        async { read.recv::<ReusedR>().await };
                        async { write.recv::<ReusedS>().await };
                    }
                    .await?;
                    assert_eq!((read, written), (7, 7));
                    assert_eq!(owner.recv::<Taken>().await?, 7);
                } else {
                    owner.send::<Abort>(&7).await?;
                }
                Ok(())
            };
            async {
                let mut r = rx.recv::<LoanR>().await?;
                let serial = r.recv::<Read>().await?;
                assert_eq!(serial, 7);
                hibana_runtime::yield_now().await;
                read_bytes.extend_from_slice(&serial.to_be_bytes());
                if rx_ok {
                    r.send::<ReadOk>(&if stale { 6 } else { serial }).await?;
                } else {
                    r.send::<ReadFailed>(&serial).await?;
                }
                rx.send::<LoanR>(r).await?;
                Ok(())
            };
            async {
                let mut s = tx.recv::<LoanS>().await?;
                let serial = s.recv::<Write>().await?;
                assert_eq!(serial, 7);
                hibana_runtime::yield_now().await;
                written_bytes.extend_from_slice(&serial.to_be_bytes());
                if tx_ok {
                    s.send::<WriteOk>(&serial).await?;
                } else {
                    s.send::<WriteFailed>(&serial).await?;
                }
                tx.send::<LoanS>(s).await?;
                Ok(())
            };
            async {
                let branch = next.offer().await?;
                match branch.label() {
                    6 => {
                        assert!(rx_ok && tx_ok && !stale);
                        assert_eq!(branch.recv::<Reuse>().await?, 7);
                        let mut r = next.recv::<LoanR>().await?;
                        let mut s = next.recv::<LoanS>().await?;
                        async {
                            async { r.send::<ReusedR>(&7).await };
                            async { s.send::<ReusedS>(&7).await };
                        }
                        .await?;
                        next.send::<Taken>(&7).await?;
                    }
                    7 => {
                        assert!(!rx_ok || !tx_ok || stale);
                        assert_eq!(branch.recv::<Abort>().await?, 7);
                    }
                    label => panic!("unexpected publication {label}"),
                }
                Ok(())
            };
        }
        .await?;
        assert_eq!(read_bytes, 7u64.to_be_bytes());
        assert_eq!(written_bytes, 7u64.to_be_bytes());
        assert!(network.queue_is_empty());
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn reuse_cannot_precede_actual_completion_receives() -> Result<(), EndpointError> {
    let global = choreography();
    let (p0, p1, p2, p3, p4, p5) = (
        project::<RX>(&global),
        project::<TX>(&global),
        project::<OWNER>(&global),
        project::<NEXT>(&global),
        project::<R>(&global),
        project::<S>(&global),
    );
    for (receive_read, complete_write) in [(true, false), (true, true), (false, true)] {
        let network = TestTransport::new();
        let [mut slab0, mut slab1, mut slab2, mut slab3] = [[0; 32 * 1024]; 4];
        let [mut s0, mut s1, mut s2, mut s3] = [(); 4].map(|_| SessionKitStorage::uninit());
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network.clone()).unwrap();
        let rv3 = s3.init().rendezvous(&mut slab3, network.clone()).unwrap();
        let sid = SessionId::new(0x7072);
        let mut owner = rv0.enter(sid, &p2).unwrap();
        let r = rv0.enter(sid, &p4).unwrap();
        let s = rv0.enter(sid, &p5).unwrap();
        let mut rx = rv1.enter(sid, &p0).unwrap();
        let mut tx = rv2.enter(sid, &p1).unwrap();
        let next = rv3.enter(sid, &p3).unwrap();
        owner.send::<LoanR>(r).await?;
        owner.send::<Read>(&7).await?;
        owner.send::<LoanS>(s).await?;
        owner.send::<Write>(&7).await?;
        let mut r = rx.recv::<LoanR>().await?;
        let mut s = tx.recv::<LoanS>().await?;
        assert_eq!(r.recv::<Read>().await?, 7);
        assert_eq!(s.recv::<Write>().await?, 7);
        r.send::<ReadOk>(&7).await?;
        rx.send::<LoanR>(r).await?;
        if receive_read {
            assert_eq!(owner.recv::<ReadOk>().await?, 7);
            let returned = owner.recv::<LoanR>().await?;
            core::hint::black_box(&returned);
            // Keep the returned view alive until after the rejected operation.
            if complete_write {
                s.send::<WriteOk>(&7).await?;
                tx.send::<LoanS>(s).await?;
            }
            assert!(owner.send::<Reuse>(&7).await.is_err());
        } else {
            s.send::<WriteOk>(&7).await?;
            tx.send::<LoanS>(s).await?;
            assert!(owner.send::<Reuse>(&7).await.is_err());
        }
        drop(next);
    }
    Ok(())
}

fn once<F: Future>(future: core::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn emergency_stop_does_not_wait_for_a_resident_branch_or_io_join() {
    let global = g::par(
        g::send::<0, 1, Msg<20, ()>>().roll(),
        g::par(
            g::send::<2, 3, Msg<21, ()>>(),
            g::seq(
                g::send::<4, 3, Msg<22, ()>>(),
                g::send::<3, 5, Msg<23, ()>>(),
            ),
        ),
    );
    let rp: RoleProgram<0> = project(&global);
    let cp: RoleProgram<1> = project(&global);
    let sp: RoleProgram<2> = project(&global);
    let op: RoleProgram<3> = project(&global);
    let ip: RoleProgram<4> = project(&global);
    let np: RoleProgram<5> = project(&global);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, TestTransport::new())
        .unwrap();
    let sid = SessionId::new(0x7071);
    let mut resident = rv.enter(sid, &rp).unwrap();
    let mut command = rv.enter(sid, &cp).unwrap();
    let mut stop = rv.enter(sid, &sp).unwrap();
    let mut owner = rv.enter(sid, &op).unwrap();
    let mut io = rv.enter(sid, &ip).unwrap();
    let mut next = rv.enter(sid, &np).unwrap();
    {
        // Model the independent native stop before any communication. This
        // assertion is about the integration's ordering, not hardware timing.
        stop.send::<Msg<21, ()>>(&()).await.unwrap();
        owner.recv::<Msg<21, ()>>().await.unwrap();
        {
            let mut waiting = pin!(owner.recv::<Msg<22, ()>>());
            assert!(once(waiting.as_mut()).is_pending());
            // A resident command still makes progress while finite cleanup waits.
            resident.send::<Msg<20, ()>>(&()).await.unwrap();
            command.recv::<Msg<20, ()>>().await.unwrap();
            io.send::<Msg<22, ()>>(&()).await.unwrap();
            waiting.await.unwrap();
        }
        owner.send::<Msg<23, ()>>(&()).await.unwrap();
        next.recv::<Msg<23, ()>>().await.unwrap();
        // Reuse did not require termination of the resident roll.
        resident.send::<Msg<20, ()>>(&()).await.unwrap();
        command.recv::<Msg<20, ()>>().await.unwrap();
    }
}

static_assertions::assert_not_impl_any!(Endpoint<'static,R>:Clone,Copy);
static_assertions::assert_not_impl_any!(Endpoint<'static,S>:Clone,Copy);
