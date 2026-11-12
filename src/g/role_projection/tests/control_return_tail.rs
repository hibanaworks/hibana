//! A returned authority and an earlier-arriving stop follow repeated resource work.
use super::*;
use crate::{
    endpoint::proof_snapshot::actor_snapshot,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::transport::TestTransport,
};

#[hibana_runtime::main(crate::test_support::in_process::InProcess::default())]
#[test]
async fn typed_control_return_follows_a_completed_choice_and_roll()
-> Result<(), crate::EndpointError> {
    type Control<'r> = g::Msg<11, Endpoint<'r, 11>>;
    type Resource<'r> = g::Msg<10, Endpoint<'r, 13>>;
    type Intent = g::Msg<80, ()>;
    type Other = g::Msg<81, ()>;
    type Request = g::Msg<12, u32>;
    type Reply = g::Msg<13, u32>;
    type Stop = g::Msg<234, ()>;
    let work = g::seq(
        g::send::<11, 7, Intent>(),
        g::seq(
            g::send::<7, 0, Resource>(),
            g::seq(
                g::send::<13, 7, Request>(),
                g::seq(g::send::<7, 13, Reply>(), g::send::<0, 7, Resource>()),
            ),
        ),
    );
    let finish = g::seq(
        g::send::<2, 0, Control>(),
        g::seq(
            g::send::<0, 1, Control>(),
            g::seq(g::send::<1, 7, Control>(), g::send::<2, 7, Stop>()),
        ),
    );
    let global = g::seq(
        g::send::<7, 2, Control>(),
        g::seq(
            g::route(
                work.roll(),
                g::seq(g::send::<11, 7, Other>(), g::send::<7, 0, Other>()),
            ),
            finish,
        ),
    );
    let p7 = crate::runtime::program::project::<7>(&global);
    let p2 = crate::runtime::program::project::<2>(&global);
    let p0 = crate::runtime::program::project::<0>(&global);
    let p1 = crate::runtime::program::project::<1>(&global);
    let p11 = crate::runtime::program::project::<11>(&global);
    let p13 = crate::runtime::program::project::<13>(&global);
    for visits in 1..=3 {
        let network = TestTransport::new();
        let mut slab = std::vec![0; 96 * 1024];
        let mut storage = SessionKitStorage::uninit();
        let rv = storage.init().rendezvous(&mut slab, network).unwrap();
        let sid = SessionId::new(9650 + visits);
        let mut owner = rv.enter(sid, &p7).unwrap();
        let resource = rv.enter(sid, &p13).unwrap();
        let control = rv.enter(sid, &p11).unwrap();
        let mut carrier = rv.enter(sid, &p2).unwrap();
        let mut worker = rv.enter(sid, &p0).unwrap();
        let mut relay = rv.enter(sid, &p1).unwrap();
        async {
            async {
                owner.send::<Control>(control).await?;
                let mut resource = resource;
                for _ in 0..visits {
                    owner.offer().await?.recv::<Intent>().await?;
                    owner.send::<Resource>(resource).await?;
                    let value = owner.recv::<Request>().await?;
                    owner.send::<Reply>(&(value + 1)).await?;
                    resource = owner.recv::<Resource>().await?;
                }
                // SAFETY: this TestTransport actor is live, between polls.
                let before = unsafe { actor_snapshot(&owner) };
                let control = crate::test_support::in_process::timeout(
                    core::time::Duration::from_secs(2),
                    owner.recv::<Control>(),
                )
                .await
                .unwrap_or_else(|error| panic!("typed return: {error:?}; {before:?}"))?;
                owner.recv::<Stop>().await?;
                Ok::<_, crate::EndpointError>((resource, control))
            };
            async {
                let mut control = carrier.recv::<Control>().await?;
                for _ in 0..visits {
                    control.send::<Intent>(&()).await?;
                }
                carrier.send::<Control>(control).await?;
                carrier.send::<Stop>(&()).await?;
            };
            async {
                for value in 0..visits {
                    let mut resource = worker.recv::<Resource>().await?;
                    resource.send::<Request>(&value).await?;
                    assert_eq!(resource.recv::<Reply>().await?, value + 1);
                    worker.send::<Resource>(resource).await?;
                }
                let control = worker.recv::<Control>().await?;
                worker.send::<Control>(control).await?;
            };
            async {
                let control = relay.recv::<Control>().await?;
                relay.send::<Control>(control).await?;
            };
            Ok(())
        }
        .await?;
    }
    Ok(())
}
