//! The receiver can use and return a resource before the sender publishes
//! its local commit. Only the owned future, never its caller, retains the
//! old value during that window.
use super::*;
use crate::endpoint::proof_snapshot::{ViewTagObservation, view_tag};
use core::{cell::Cell, future::poll_fn, pin::Pin};
use std::{cell::RefCell, time::Instant};

type Global = g::Seq<
    g::Send<0, 1, Loan>,
    g::Seq<
        g::Send<10, 1, Read>,
        g::Seq<
            g::Send<1, 10, Value>,
            g::Seq<g::Send<1, 0, Loan>, g::Seq<g::Send<10, 0, Read>, g::Send<0, 10, Value>>>,
        >,
    >,
>;
const LANES: usize = g::ProgramProjection::<Global, 16>::SOURCE_EFF_LIST.len();
const SID: SessionId = SessionId::new(9016);
const WORKER: &str = "g::role_projection::tests::native::preemption::worker_process";

#[tokio::test(flavor = "current_thread")]
async fn receiver_finishes_before_the_sender_publishes_its_transfer() {
    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mailboxes = Mailboxes::new(&[0, 1, 10], LANES).unwrap();
    // SAFETY: this test owns the resource namespace and the only initial
    // issuer. The child is reaped before its queues and binding are destroyed.
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
    // SAFETY: only async-signal-safe fcntl runs between fork and exec. Each
    // descriptor is owned and all unrelated descriptors remain close-on-exec.
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
    let child = RefCell::new(ChildGuard(Some(command.spawn().unwrap())));
    drop(inherited);

    // These copied tags observe the sender's header; they cannot make an
    // Endpoint. No test observation authorizes an application operation.
    let sender_tag: Cell<Option<ViewTagObservation>> = Cell::new(None);
    let before_publication = |header: crate::runtime::transport::FrameHeader| {
        if header.bytes()[5..7] != [0, 1] {
            return Ok(());
        }
        let tag = sender_tag.get().unwrap();
        // SAFETY: the pinned sender runtime and its future-owned DB value are
        // alive throughout this synchronous transport observation.
        assert!(unsafe { tag.matches_header() });
        // SAFETY: this exact pinned runtime uses NativeTransport. No progress
        // borrow is attempted while the outer send callback holds its lease.
        let cut = unsafe { tag.actor_call_cut::<NativeTransport<'_>>() };
        assert!(!cut.lease_available);
        assert_eq!(cut.fault, None);
        let mut child = child.borrow_mut();
        let start = Instant::now();
        while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(start.elapsed() < Duration::from_secs(3), "receiver stopped");
            std::thread::sleep(Duration::from_millis(1));
        }
        let output = child.0.take().unwrap().wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for line in String::from_utf8(output.stdout).unwrap().lines() {
            std::eprintln!("preempting receiver: {line}");
        }
        // The receiver already completed its Endpoint operations and exited.
        // The old sender header is still live until poll_send returns below;
        // that header alone is therefore not a usable application capability.
        // SAFETY: the same pinned sender allocation remains alive.
        assert!(unsafe { tag.matches_header() });
        // SAFETY: the sender is still inside the same leased poll.
        let cut = unsafe { tag.actor_call_cut::<NativeTransport<'_>>() };
        assert!(!cut.lease_available);
        assert_eq!(cut.fault, None);
        Ok(())
    };
    let mut io = NativeTransport::new(
        mailboxes.directory(),
        mailboxes.descriptors(&[0, 10]).unwrap(),
        Some((10, &binding)),
    )
    .unwrap();
    io.after_send_acceptance(&before_publication);
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, io).unwrap();
    let mut owner = rv.enter(SID, &owner_program).unwrap();
    let db = rv.enter(SID, &db_program).unwrap();
    sender_tag.set(Some(view_tag(&db)));

    let mut transfer = owner.send::<Loan>(db);
    poll_fn(|cx| {
        let result = Pin::new(&mut transfer).poll(cx);
        if child.borrow().0.is_none() {
            assert!(
                result.is_ready(),
                "a physically accepted send must finish its publication in this poll"
            );
        }
        result
    })
    .await
    .unwrap();
    assert!(
        child.borrow().0.is_none(),
        "the receiver must finish inside the transport's send poll"
    );
    let old_tag = sender_tag.get().unwrap();
    // SAFETY: storage owns this pinned view record for the whole run.
    assert!(!unsafe { old_tag.matches_header() });
    // SAFETY: storage and the owner's Endpoint retain this exact actor.
    let cut = unsafe { old_tag.actor_call_cut::<NativeTransport<'_>>() };
    assert!(cut.lease_available);
    assert_eq!(cut.fault, None);
    drop(transfer);
    let mut db = owner.recv::<Loan>().await.unwrap();
    // Issuing the returned continuation cannot resurrect the old tag.
    // SAFETY: the same view record remains pinned and owned by this runtime.
    assert!(!unsafe { old_tag.matches_header() });
    tokio::join!(
        async {
            db.send::<Read>(&42).await.unwrap();
            assert_eq!(db.recv::<Value>().await.unwrap(), 43);
        },
        async {
            assert_eq!(owner.recv::<Read>().await.unwrap(), 42);
            owner.send::<Value>(&43).await.unwrap();
        },
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_accepted_transport_error_releases_only_a_faulted_actor() {
    use crate::{rendezvous::SessionFaultKind, runtime::transport::TransportError};

    let () = g::ProgramProjection::<Global, 16>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 16>::IMAGE_REF,
    );
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 16>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 16>::IMAGE_REF,
    );
    let mailboxes = Mailboxes::new(&[0, 1, 10], LANES).unwrap();
    // SAFETY: this test owns the only issuer and the entire physical queue
    // namespace. Both runtimes are destroyed before that namespace is removed.
    let binding = unsafe { crate::runtime::transport::EndpointBinding::new() };
    let sender_tag: Cell<Option<ViewTagObservation>> = Cell::new(None);
    let accepted_error = |header: crate::runtime::transport::FrameHeader| {
        assert_eq!(header.bytes()[5..7], [0, 1]);
        let tag = sender_tag.get().unwrap();
        // SAFETY: this copied tag names the live future-owned value in this
        // exact pinned NativeTransport actor, inside its accepted send poll.
        assert!(unsafe { tag.matches_header() });
        let cut = unsafe { tag.actor_call_cut::<NativeTransport<'_>>() };
        assert!(!cut.lease_available);
        assert_eq!(cut.fault, None);
        Err(TransportError::Failed)
    };
    let mut owner_io = NativeTransport::new(
        mailboxes.directory(),
        mailboxes.descriptors(&[0, 10]).unwrap(),
        Some((10, &binding)),
    )
    .unwrap();
    owner_io.after_send_acceptance(&accepted_error);
    let worker_io = NativeTransport::new(
        mailboxes.directory(),
        mailboxes.descriptors(&[1, 10]).unwrap(),
        None,
    )
    .unwrap();
    let mut owner_slab = vec![0; 64 * 1024];
    let mut worker_slab = vec![0; 64 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, owner_io)
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, worker_io)
        .unwrap();
    let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
    let db = owner_rv.enter(SID, &db_program).unwrap();
    let mut worker = worker_rv.enter(SID, &worker_program).unwrap();
    sender_tag.set(Some(view_tag(&db)));
    let mut transfer = owner.send::<Loan>(db);
    (&mut transfer).await.unwrap_err();
    let tag = sender_tag.get().unwrap();
    // SAFETY: the owner and completed future retain the pinned actor. An
    // accepted-but-failed send may retain its old raw header, but this actor
    // cannot admit further live-session operations after releasing its lease.
    let cut = unsafe { tag.actor_call_cut::<NativeTransport<'_>>() };
    assert!(cut.lease_available);
    assert_eq!(cut.fault, Some(SessionFaultKind::TransportClosed));

    // The datagram really was accepted. A second runtime can receive and use
    // the sole right despite the sender's uncertain-delivery error.
    let mut db = worker.recv::<Loan>().await.unwrap();
    tokio::join!(
        async {
            db.send::<Read>(&41).await.unwrap();
            assert_eq!(db.recv::<Value>().await.unwrap(), 42);
        },
        async {
            assert_eq!(worker.recv::<Read>().await.unwrap(), 41);
            worker.send::<Value>(&42).await.unwrap();
        },
    );
    worker.send::<Loan>(db).await.unwrap();
    drop(transfer);
    assert!(owner.recv::<Loan>().await.is_err());
    // SAFETY: the owner still retains the same actor after rejecting receipt.
    let cut = unsafe { tag.actor_call_cut::<NativeTransport<'_>>() };
    assert_eq!(cut.fault, Some(SessionFaultKind::TransportClosed));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process entrypoint exercised by the preemption proof"]
async fn worker_process() {
    let parent: u32 = std::env::var("HIBANA_NATIVE_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(parent, std::process::id());
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
            // SAFETY: exec inherited each descriptor uniquely with its number
            // intact, and no other child object owns it.
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
    tokio::join!(
        async {
            db.send::<Read>(&41).await.unwrap();
            assert_eq!(db.recv::<Value>().await.unwrap(), 42);
        },
        async {
            assert_eq!(worker.recv::<Read>().await.unwrap(), 41);
            worker.send::<Value>(&42).await.unwrap();
        },
    );
    worker.send::<Loan>(db).await.unwrap();
    std::println!("pid={} receiver used and returned DB", std::process::id());
}
