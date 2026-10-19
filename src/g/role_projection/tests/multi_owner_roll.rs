use super::*;
use crate::{
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

type X = Endpoint<'static, 10>;
type Y = Endpoint<'static, 11>;
type LoanX = g::Msg<141, X>;
type LoanY = g::Msg<142, Y>;
type ReturnX = g::Msg<143, X>;
type ReturnY = g::Msg<144, Y>;
type TailLoanY = g::Msg<151, Y>;
type TailAck = g::Msg<152, ()>;
type TailLoanX = g::Msg<153, X>;
type TailReturnY = g::Msg<154, Y>;
type TailReturnX = g::Msg<155, X>;
type TailReturned = g::Msg<156, ()>;
type TailReady = g::Msg<157, ()>;
type TailForwardY = g::Msg<158, Y>;
type Compute = g::Msg<159, u32>;
type Body = g::Seq<
    g::Send<0, 1, LoanX>,
    g::Seq<g::Send<1, 0, LoanY>, g::Seq<g::Send<1, 0, ReturnX>, g::Send<0, 1, ReturnY>>>,
>;
type UnsafeTail = g::Seq<
    g::Send<1, 2, TailLoanY>,
    g::Seq<
        g::Send<2, 0, TailAck>,
        g::Seq<
            g::Send<0, 2, TailLoanX>,
            g::Seq<g::Send<2, 1, TailReturnY>, g::Send<2, 0, TailReturnX>>,
        >,
    >,
>;
type GuardedTail = g::Seq<
    g::Send<0, 2, TailLoanX>,
    g::Seq<
        g::Send<2, 0, TailAck>,
        g::Seq<
            g::Send<0, 1, TailReady>,
            g::Seq<
                g::Send<1, 2, TailLoanY>,
                g::Seq<
                    g::Send<10, 2, Compute>,
                    g::Seq<
                        g::Send<2, 0, TailReturnY>,
                        g::Seq<g::Send<0, 1, TailForwardY>, g::Send<2, 0, TailReturnX>>,
                    >,
                >,
            >,
        >,
    >,
>;
type Steps = g::Seq<g::Roll<Body>, GuardedTail>;

#[test]
fn temporary_resource_departure_needs_reentry_knowledge() {
    let diagnostic = g::ProgramProjection::<g::Seq<g::Roll<Body>, UnsafeTail>, 16>::DIAGNOSTIC
        .expect("a return at the end cannot authorize earlier reentry");
    assert_eq!(
        diagnostic.problem,
        g::ProjectionProblem::ReentryResourceKnowledge
    );
}

#[test]
fn an_accepted_return_can_wait_for_its_independent_receipt() {
    type EarlyRelease = g::Seq<
        g::Send<0, 2, TailLoanX>,
        g::Seq<
            g::Send<2, 0, TailAck>,
            g::Seq<
                g::Send<0, 1, TailReady>,
                g::Seq<
                    g::Send<1, 2, TailLoanY>,
                    g::Seq<
                        g::Send<2, 0, TailReturnY>,
                        g::Seq<
                            g::Send<0, 1, TailForwardY>,
                            g::Seq<g::Send<0, 2, TailReturned>, g::Send<2, 0, TailReturnX>>,
                        >,
                    >,
                >,
            >,
        >,
    >;
    type Protocol = g::Seq<g::Roll<Body>, EarlyRelease>;
    let source = const { crate::g::source::ProgramSourceData::<16>::lower::<Protocol>() };
    assert!(
        source
            .actors()
            .unwrap()
            .has_reentry_knowledge(&<Protocol as g::ProgramShape>::SOURCE_NODE,)
    );
}

#[test]
fn a_guard_cannot_come_back_before_another_loan_needed_to_return_the_resource() {
    type Released = g::Seq<
        g::Send<2, 0, TailReturnX>,
        g::Seq<
            g::Send<0, 2, TailLoanX>,
            g::Seq<g::Send<2, 1, TailReturnY>, g::Send<2, 0, TailReturnX>>,
        >,
    >;
    type EarlyRelease = g::Seq<
        g::Send<0, 2, TailLoanX>,
        g::Seq<
            g::Send<2, 0, TailAck>,
            g::Seq<g::Send<0, 1, TailReady>, g::Seq<g::Send<1, 2, TailLoanY>, Released>>,
        >,
    >;
    type Protocol = g::Seq<g::Roll<Body>, EarlyRelease>;
    let source = const { crate::g::source::ProgramSourceData::<16>::lower::<Protocol>() };
    assert!(
        !source
            .actors()
            .unwrap()
            .has_reentry_knowledge(&<Protocol as g::ProgramShape>::SOURCE_NODE,)
    );
}

#[test]
fn another_local_entry_resource_can_guard_a_remote_resource_excursion() {
    let () = g::ProgramProjection::<Steps, 16>::VALIDATION;
    let a_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Steps, 16>::IMAGE_REF,
    );
    let b_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Steps, 16>::IMAGE_REF,
    );
    let c_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Steps, 16>::IMAGE_REF,
    );
    let x_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Steps, 16>::IMAGE_REF,
    );
    let y_program = crate::global::role_program::role_program_from_image::<11>(
        &RoleProjection::<11, Steps, 16>::IMAGE_REF,
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let network = TestTransport::new();
            let mut a_slab = [0; 48 * 1024];
            let mut b_slab = [0; 48 * 1024];
            let mut c_slab = [0; 48 * 1024];
            let mut a_storage = SessionKitStorage::uninit();
            let mut b_storage = SessionKitStorage::uninit();
            let mut c_storage = SessionKitStorage::uninit();
            let a_rv = a_storage
                .init()
                .rendezvous(&mut a_slab, network.clone())
                .unwrap();
            let b_rv = b_storage
                .init()
                .rendezvous(&mut b_slab, network.clone())
                .unwrap();
            let c_rv = c_storage
                .init()
                .rendezvous(&mut c_slab, network.clone())
                .unwrap();
            let sid = SessionId::new(9071);
            let mut a = a_rv.enter(sid, &a_program).unwrap();
            let mut b = b_rv.enter(sid, &b_program).unwrap();
            let mut c = c_rv.enter(sid, &c_program).unwrap();
            let x = a_rv.enter(sid, &x_program).unwrap();
            let y = b_rv.enter(sid, &y_program).unwrap();
            let execution = async {
                tokio::join!(
                    async {
                        a.send::<LoanX>(x).await.unwrap();
                        let y = a.recv::<LoanY>().await.unwrap();
                        let x = a.recv::<ReturnX>().await.unwrap();
                        a.send::<ReturnY>(y).await.unwrap();
                        a.send::<TailLoanX>(x).await.unwrap();
                        a.recv::<TailAck>().await.unwrap();
                        a.send::<TailReady>(&()).await.unwrap();
                        let y = a.recv::<TailReturnY>().await.unwrap();
                        a.send::<TailForwardY>(y).await.unwrap();
                        let x = a.recv::<TailReturnX>().await.unwrap();
                        a.send::<LoanX>(x).await.unwrap();
                        let y = a.recv::<LoanY>().await.unwrap();
                        let x = a.recv::<ReturnX>().await.unwrap();
                        a.send::<ReturnY>(y).await.unwrap();
                        x
                    },
                    async {
                        let x = b.recv::<LoanX>().await.unwrap();
                        b.send::<LoanY>(y).await.unwrap();
                        b.send::<ReturnX>(x).await.unwrap();
                        let y = b.recv::<ReturnY>().await.unwrap();
                        b.recv::<TailReady>().await.unwrap();
                        b.send::<TailLoanY>(y).await.unwrap();
                        let y = b.recv::<TailForwardY>().await.unwrap();
                        let x = b.recv::<LoanX>().await.unwrap();
                        b.send::<LoanY>(y).await.unwrap();
                        b.send::<ReturnX>(x).await.unwrap();
                        b.recv::<ReturnY>().await.unwrap()
                    },
                    async {
                        let mut x = c.recv::<TailLoanX>().await.unwrap();
                        c.send::<TailAck>(&()).await.unwrap();
                        let y = c.recv::<TailLoanY>().await.unwrap();
                        x.send::<Compute>(&9).await.unwrap();
                        assert_eq!(c.recv::<Compute>().await.unwrap(), 9);
                        c.send::<TailReturnY>(y).await.unwrap();
                        c.send::<TailReturnX>(x).await.unwrap();
                    },
                )
            };
            let (x, y, ()) =
                tokio::time::timeout(core::time::Duration::from_millis(100), execution)
                    .await
                    .expect("the guarded continuation and next visit must complete");
            core::hint::black_box((x, y));
        });
}
