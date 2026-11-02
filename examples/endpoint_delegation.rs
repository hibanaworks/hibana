#[path = "support/in_memory.rs"]
mod in_memory;
#[path = "support/in_process.rs"]
pub mod in_process;

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

#[hibana_runtime::main(in_process::InProcess::default())]
async fn main() -> core::result::Result<(), hibana::EndpointError> {
    // DB names the operation right. SERVICE owns the concrete computation.
    // The role and continuation of DB survive both transfers.
    let global = g::seq(
        g::send::<OWNER, WORKER, Loan>(),
        g::seq(
            g::par(
                g::seq(
                    g::send::<DB, SERVICE, Read>(),
                    g::send::<SERVICE, DB, Value>(),
                ),
                g::send::<DB, AUDIT, Record>(),
            ),
            g::seq(
                g::send::<WORKER, OWNER, Loan>(),
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
    let session = SessionId::new(1);
    let network = InMemoryTransport::new([((session, DB), binding)]);
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
    let mut owner = owner_runtime.enter(session, &owner_program).unwrap();
    let db = owner_runtime.enter(session, &db_program).unwrap();
    let mut worker = worker_runtime.enter(session, &worker_program).unwrap();
    let mut service = service_runtime.enter(session, &service_program).unwrap();
    let mut audit = audit_runtime.enter(session, &audit_program).unwrap();

    let (answer, worker_answer, (), record) = async {
        async {
            owner.send::<Loan>(db).await?;
            let mut db = owner.recv::<Loan>().await?;
            db.send::<Read>(&41).await?;
            db.recv::<Value>().await
        };
        async {
            let mut db = worker.recv::<Loan>().await?;
            let (answer, ()) = async {
                let [mut query, mut record] = db.par()?;
                async {
                    query.send::<Read>(&7).await?;
                    query.recv::<Value>().await
                };
                async { record.send::<Record>(&7).await };
            }
            .await?;
            worker.send::<Loan>(db).await?;
            Ok(answer)
        };
        async {
            let key = service.recv::<Read>().await?;
            service.send::<Value>(&(key + 1)).await?;
            let key = service.recv::<Read>().await?;
            service.send::<Value>(&(key + 1)).await?;
        };
        async { audit.recv::<Record>().await };
    }
    .await?;
    assert_eq!((worker_answer, answer, record), (8, 42, 7));
    println!("worker={worker_answer}, owner={answer}, audit={record}");
    Ok(())
}
