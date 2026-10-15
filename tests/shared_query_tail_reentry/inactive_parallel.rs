//! An unchosen branch may leave owner conditional parallel dependency on the chosen
//! continuation. Its inactive prerequisite must not erase that continuation.
use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};
const OWNER: u8 = 0;
const CLIENT: u8 = 1;
const REVIEWER: u8 = 2;

type Outer = g::Msg<1, ()>;
type Inner = g::Msg<2, ()>;
type RequestReady = g::Msg<3, ()>;
type Request = g::Msg<4, ()>;
type Open = g::Msg<15, ()>;
type Opened = g::Msg<16, ()>;
type Admitted = g::Msg<5, ()>;
type Validate = g::Msg<17, ()>;
type Rejected = g::Msg<18, ()>;
type Result = g::Msg<6, ()>;
type Done = g::Msg<7, ()>;
type CloseReady = g::Msg<29, ()>;
type CloseExit = g::Msg<30, ()>;
type Merge = g::Msg<19, ()>;
type BeginReview = g::Msg<8, ()>;
type ReviewRequested = g::Msg<9, ()>;
type CanReview = g::Msg<10, ()>;
type Revise = g::Msg<11, ()>;
type Approved = g::Msg<12, ()>;
type ReviseFeedback = g::Msg<13, ()>;
type ApproveFeedback = g::Msg<14, ()>;
#[tokio::test(flavor = "current_thread")]
async fn inactive_parallel_dependency_does_not_block_outer_roll_reentry() {
    let body = g::seq(
        g::send::<OWNER, OWNER, Inner>(),
        g::seq(
            g::send::<OWNER, CLIENT, RequestReady>(),
            g::route(
                g::seq(
                    g::send::<CLIENT, OWNER, Open>(),
                    g::send::<OWNER, CLIENT, Opened>(),
                ),
                g::seq(
                    g::send::<CLIENT, OWNER, Request>(),
                    g::route(
                        g::seq(
                            g::send::<OWNER, OWNER, Validate>(),
                            g::send::<OWNER, CLIENT, Admitted>(),
                        ),
                        g::send::<OWNER, CLIENT, Rejected>(),
                    ),
                ),
            ),
        ),
    )
    .roll();
    let review = g::seq(
        g::send::<REVIEWER, REVIEWER, BeginReview>(),
        g::send::<REVIEWER, OWNER, ReviewRequested>(),
    );
    let prefix = g::par(g::seq(g::send::<OWNER, OWNER, Outer>(), body), review);
    let merge = g::seq(
        g::send::<OWNER, OWNER, Merge>(),
        g::seq(
            g::send::<OWNER, CLIENT, RequestReady>(),
            g::seq(
                g::send::<CLIENT, OWNER, Request>(),
                g::send::<OWNER, CLIENT, Admitted>(),
            ),
        ),
    )
    .roll();
    let accepted = g::seq(
        g::send::<REVIEWER, REVIEWER, Approved>(),
        g::seq(
            g::send::<REVIEWER, OWNER, ApproveFeedback>(),
            g::seq(
                g::send::<OWNER, CLIENT, Result>(),
                g::seq(
                    g::send::<OWNER, CLIENT, Done>(),
                    g::par(
                        merge,
                        g::seq(
                            g::send::<REVIEWER, CLIENT, CloseRequestReady>(),
                            g::send::<CLIENT, REVIEWER, CloseExit>(),
                        )
                        .roll(),
                    ),
                ),
            ),
        ),
    );
    let rejected = g::seq(
        g::send::<REVIEWER, REVIEWER, Revise>(),
        g::seq(
            g::send::<REVIEWER, OWNER, ReviseFeedback>(),
            g::seq(
                g::send::<OWNER, CLIENT, Result>(),
                g::send::<OWNER, CLIENT, Done>(),
            ),
        ),
    );
    let global = g::seq(
        prefix,
        g::seq(
            g::send::<OWNER, REVIEWER, CanReview>(),
            g::route(accepted, rejected),
        ),
    )
    .roll();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let mut bytes = vec![0; 1024 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let kit = storage.init();
    let carrier = crate::common::TestTransport::new();
    let rv = kit.rendezvous(&mut bytes, carrier.clone()).unwrap();
    let mut owner = rv.enter(SessionId::new(1), &p0).unwrap();
    let mut client = rv.enter(SessionId::new(1), &p1).unwrap();
    let mut reviewer = rv.enter(SessionId::new(1), &p2).unwrap();
    tokio::join!(
        async {
            for _ in 0..3 {
                owner.send::<Outer>(&()).await.expect("owner outer");
                owner.send::<Inner>(&()).await.expect("owner inner open");
                owner
                    .send::<RequestReady>(&())
                    .await
                    .expect("owner ready open");
                owner.recv::<Open>().await.expect("owner open");
                owner.send::<Opened>(&()).await.expect("owner opened");
                owner.send::<Inner>(&()).await.expect("owner inner");
                owner.send::<RequestReady>(&()).await.expect("owner ready");
                owner.recv::<Request>().await.expect("owner request");
                owner.send::<Validate>(&()).await.expect("owner validate");
                owner.send::<Admitted>(&()).await.expect("owner admitted");
                owner
                    .recv::<ReviewRequested>()
                    .await
                    .expect("owner review requested");
                owner
                    .send::<CanReview>(&())
                    .await
                    .expect("owner can review");
                owner
                    .recv::<ReviseFeedback>()
                    .await
                    .expect("owner feedback");
                owner.send::<Result>(&()).await.expect("owner result");
                owner.send::<Done>(&()).await.expect("owner done");
            }
        },
        async {
            for _ in 0..3 {
                client
                    .recv::<RequestReady>()
                    .await
                    .expect("client ready open");
                client.send::<Open>(&()).await.expect("client open");
                client.recv::<Opened>().await.expect("client opened");
                client.recv::<RequestReady>().await.expect("client ready");
                client.send::<Request>(&()).await.expect("client request");
                client
                    .offer()
                    .await
                    .expect("client offer admitted")
                    .recv::<Admitted>()
                    .await
                    .expect("client admitted");
                client.recv::<Result>().await.expect("client result");
                client.recv::<Done>().await.expect("client done");
            }
        },
        async {
            for _ in 0..3 {
                reviewer
                    .send::<BeginReview>(&())
                    .await
                    .expect("reviewer begin review");
                reviewer
                    .send::<ReviewRequested>(&())
                    .await
                    .expect("reviewer review requested");
                reviewer
                    .recv::<CanReview>()
                    .await
                    .expect("reviewer can review");
                reviewer.send::<Revise>(&()).await.expect("reviewer revise");
                reviewer
                    .send::<ReviseFeedback>(&())
                    .await
                    .expect("reviewer revise feedback");
            }
        }
    );
}
