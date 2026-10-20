#[path = "support/in_memory.rs"]
mod in_memory;

use hibana::{
    Endpoint, g,
    runtime::{SessionKitStorage, ids::SessionId, program::project, transport::EndpointBinding},
};
use in_memory::InMemoryTransport;

const OWNER: u8 = 0;
const WORKER: u8 = 1;
const DB: u8 = 2;
const SERVICE: u8 = 3;
const AUDIT: u8 = 4;
type Loan<'r> = g::Msg<1, Endpoint<'r, DB>>;
type Read = g::Msg<2, u32>;
type Value = g::Msg<3, u32>;
type Record = g::Msg<4, u32>;

fn main() {
    // DB names the operation right. SERVICE owns the concrete computation.
    // The role and continuation of DB survive both transfers.
    let global = g::seq(
        g::send::<OWNER, WORKER, Loan<'_>>(),
        g::seq(
            g::par(
                g::seq(
                    g::send::<DB, SERVICE, Read>(),
                    g::send::<SERVICE, DB, Value>(),
                ),
                g::send::<DB, AUDIT, Record>(),
            ),
            g::seq(
                g::send::<WORKER, OWNER, Loan<'_>>(),
                g::seq(
                    g::send::<DB, SERVICE, Read>(),
                    g::send::<SERVICE, DB, Value>(),
                ),
            ),
        ),
    );
    let owner_program = project::<OWNER>(&global);
    let worker_program = project::<WORKER>(&global);
    let db_program = project::<DB>(&global);
    let service_program = project::<SERVICE>(&global);
    let audit_program = project::<AUDIT>(&global);

    // SAFETY: this example has one physical service and one initial DB issuer.
    // Every runtime below shares this same binding; no other path issues DB.
    // The carrier keeps one logical inbox across holder changes. This host
    // example is not an authenticated network transport.
    let binding = unsafe { EndpointBinding::new() };
    let network = InMemoryTransport::new([(DB, binding)]);
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut service_slab = [0; 32 * 1024];
    let mut audit_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let mut audit_storage = SessionKitStorage::uninit();
    let owner_runtime = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_runtime = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    let service_runtime = service_storage
        .init()
        .rendezvous(&mut service_slab, network.clone())
        .unwrap();
    let audit_runtime = audit_storage
        .init()
        .rendezvous(&mut audit_slab, network)
        .unwrap();
    let session = SessionId::new(1);
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let db = owner_runtime.enter(session, &db_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    let mut service = service_runtime.enter(session, &service_program).unwrap();
    let mut audit = audit_runtime.enter(session, &audit_program).unwrap();

    // The carrier uses in-memory readiness, so no OS I/O driver is needed.
    // The one Tokio executor polls all four localsides and both par fragments.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (answer, worker_answer, (), record) = runtime.block_on(async {
        tokio::join!(
            async {
                owner.send::<Loan<'_>>(db).await.unwrap();
                let mut db = owner.recv::<Loan<'_>>().await.unwrap();
                db.send::<Read>(&41).await.unwrap();
                db.recv::<Value>().await.unwrap()
            },
            async {
                let mut db = worker.recv::<Loan<'_>>().await.unwrap();
                let answer = {
                    let [mut query, mut record] = db.par().unwrap();
                    let (answer, ()) = tokio::join!(
                        async {
                            query.send::<Read>(&7).await.unwrap();
                            query.recv::<Value>().await.unwrap()
                        },
                        async { record.send::<Record>(&7).await.unwrap() },
                    );
                    answer
                };
                worker.send::<Loan<'_>>(db).await.unwrap();
                answer
            },
            async {
                let key = service.recv::<Read>().await.unwrap();
                service.send::<Value>(&(key + 1)).await.unwrap();
                let key = service.recv::<Read>().await.unwrap();
                service.send::<Value>(&(key + 1)).await.unwrap();
            },
            async { audit.recv::<Record>().await.unwrap() },
        )
    });
    assert_eq!((worker_answer, answer, record), (8, 42, 7));
    println!("worker={worker_answer}, owner={answer}, audit={record}");
}
