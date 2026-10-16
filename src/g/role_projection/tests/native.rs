//! Real processes share only kernel datagram queues, never runtime memory.
//! The actor program remains the production image; only the unfinished public
//! owned-acceptance guard is bypassed, as in the other owned execution tests.
use super::{Loan, Read, RoleProjection, Value};
use crate::{
    g,
    runtime::{SessionKitStorage, ids::SessionId},
};
use std::{
    fs::OpenOptions,
    io::{Read as _, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{net::UnixDatagram, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    string::{String, ToString},
    time::Duration,
    vec,
    vec::Vec,
};

#[path = "../../../../tests/support/native_mailbox.rs"]
mod mailbox;
use mailbox::{Mailboxes, NativeTransport};

// SERVICE grants DB to OWNER, which lends it to WORKER. After the return,
// OWNER continues DB's protocol and returns the finished right to SERVICE.
type Global = g::Seq<
    g::Send<2, 0, Loan>,
    g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Send<10, 2, Read>,
            g::Seq<
                g::Send<2, 10, Value>,
                g::Seq<
                    g::Send<1, 0, Loan>,
                    g::Seq<
                        g::Send<10, 2, Read>,
                        g::Seq<g::Send<2, 10, Value>, g::Send<0, 2, Loan>>,
                    >,
                >,
            >,
        >,
    >,
>;

// At most one lane per source event; provision this derived upper bound.
const LANES: usize = g::ProgramProjection::<Global, 16>::SOURCE_EFF_LIST.len();
const SID: SessionId = SessionId::new(9010);
const WORKER: &str = "g::role_projection::tests::native::worker_process";

struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            child.wait().expect("reap native proof child");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_process_returns_the_endpoint_and_exits_before_its_continuation_runs() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mailboxes = Mailboxes::new(&[0, 1, 2, 10], LANES).unwrap();
    let resource_path = mailboxes.directory().join("resource");
    let mut resource = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&resource_path)
        .unwrap();
    std::fs::remove_file(resource_path).unwrap();
    resource.write_all(&41u32.to_be_bytes()).unwrap();
    // SAFETY: this unlinked file has one service owner and no other resource
    // handles. The binding lives until the child is reaped and all runtimes
    // and inherited queues have been fenced by destruction of the namespace.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };

    let inherited = mailboxes.descriptors(&[1, 10]).unwrap();
    let descriptors = inherited
        .iter()
        .map(|((role, lane), socket)| std::format!("{role},{lane},{}", socket.as_raw_fd()))
        .collect::<Vec<_>>()
        .join(";");
    let raw_descriptors: Vec<_> = inherited
        .iter()
        .map(|(_, socket)| socket.as_raw_fd())
        .collect();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", WORKER, "--nocapture"])
        .env("HIBANA_NATIVE_DIRECTORY", mailboxes.directory())
        .env("HIBANA_NATIVE_DESCRIPTORS", descriptors)
        .env("HIBANA_NATIVE_PARENT", std::process::id().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the post-fork callback only performs async-signal-safe fcntl
    // calls on these owned, distinct descriptors. No allocation or lock is
    // used. All other descriptors keep their standard close-on-exec flag.
    unsafe {
        command.pre_exec(move || {
            for &fd in &raw_descriptors {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = ChildGuard(Some(command.spawn().unwrap()));
    drop(inherited);

    {
        let owner_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[0, 10]).unwrap(),
            None,
        )
        .unwrap();
        let service_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[2, 10]).unwrap(),
            Some((10, &binding)),
        )
        .unwrap();
        let mut owner_slab = vec![0; 64 * 1024];
        let mut service_slab = vec![0; 64 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, owner_io)
            .unwrap();
        let service_rv = service_storage
            .init()
            .rendezvous(&mut service_slab, service_io)
            .unwrap();
        assert!(
            owner_rv.enter(SID, &db_program).is_err(),
            "OS handles are not an initial resource authority"
        );
        let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
        let mut service = service_rv.enter(SID, &service_program).unwrap();
        let db = service_rv.enter(SID, &db_program).unwrap();
        tokio::join!(
            async {
                service.send::<Loan>(db).await.unwrap();
                let delta = service.recv::<Read>().await.unwrap();
                let mut bytes = [0; 4];
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.read_exact(&mut bytes).unwrap();
                let value = u32::from_be_bytes(bytes) + delta;
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.write_all(&value.to_be_bytes()).unwrap();
                service.send::<Value>(&value).await.unwrap();

                let delta = service.recv::<Read>().await.unwrap();
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.read_exact(&mut bytes).unwrap();
                let value = u32::from_be_bytes(bytes) + delta;
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.write_all(&value.to_be_bytes()).unwrap();
                service.send::<Value>(&value).await.unwrap();
                drop(service.recv::<Loan>().await.unwrap());
            },
            async {
                let db = owner.recv::<Loan>().await.unwrap();
                owner.send::<Loan>(db).await.unwrap();
                let mut db = owner.recv::<Loan>().await.unwrap();

                // A test observation, not a protocol authority: establish that
                // no sender runtime or address space remains before using DB.
                tokio::time::timeout(Duration::from_secs(3), async {
                    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                })
                .await
                .unwrap();
                let output = child.0.take().unwrap().wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "native child failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                for line in String::from_utf8(output.stdout).unwrap().lines() {
                    std::eprintln!("native worker: {line}");
                }
                db.send::<Read>(&5).await.unwrap();
                assert_eq!(db.recv::<Value>().await.unwrap(), 49);
                owner.send::<Loan>(db).await.unwrap();
            }
        );
        let mut bytes = [0; 4];
        resource.seek(SeekFrom::Start(0)).unwrap();
        resource.read_exact(&mut bytes).unwrap();
        assert_eq!(u32::from_be_bytes(bytes), 49);
    }
    // Every issued view and child has retired before the native queue fence.
    // Keep the physical binding alive through that fence.
    drop(mailboxes);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process entrypoint exercised by the parent native proof"]
async fn worker_process() {
    let parent: u32 = std::env::var("HIBANA_NATIVE_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(parent, std::process::id());
    std::println!("pid={} parent={parent}", std::process::id());
    let directory = std::path::PathBuf::from(std::env::var_os("HIBANA_NATIVE_DIRECTORY").unwrap());
    let descriptors = std::env::var("HIBANA_NATIVE_DESCRIPTORS")
        .unwrap()
        .split(';')
        .map(|item| {
            let fields = item
                .split(',')
                .map(|field| field.parse::<i32>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(fields.len(), 3);
            let role = u8::try_from(fields[0]).unwrap();
            let lane = u8::try_from(fields[1]).unwrap();
            assert!(fields[2] >= 0);
            // SAFETY: the parent uniquely passed these owned descriptors through
            // exec, preserving their numbers; no other child object owns them.
            let socket = unsafe { UnixDatagram::from_raw_fd(fields[2]) };
            ((role, lane), socket)
        })
        .collect();
    let io = NativeTransport::new(&directory, descriptors, None).unwrap();
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, io).unwrap();
    assert!(rv.enter(SID, &db_program).is_err());
    let mut worker = rv.enter(SID, &worker_program).unwrap();
    let mut db = worker.recv::<Loan>().await.unwrap();
    db.send::<Read>(&3).await.unwrap();
    assert_eq!(db.recv::<Value>().await.unwrap(), 44);
    worker.send::<Loan>(db).await.unwrap();
}
