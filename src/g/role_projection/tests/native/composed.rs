//! A resource crosses a process boundary inside a rolled parallel fragment.
//! Resolver decisions are application inputs; Endpoint operations own progress.
use super::*;
use crate::runtime::resolver::{DecisionArm, ResolverRef};
use core::cell::Cell;

use super::super::composed::{AfterRead, AfterValue, Finished, Global, Subtract};
const LANES: usize = g::ProgramProjection::<Global, 32>::SOURCE_EFF_LIST.len();
const SID: SessionId = SessionId::new(9015);
const WORKER: &str = "g::role_projection::tests::native::composed::worker_process";

#[tokio::test(flavor = "current_thread")]
async fn process_handoff_preserves_resolved_choices_in_parallel_rolled_loans() {
    let () = g::ProgramProjection::<Global, 32>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 32>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 32>::IMAGE_REF,
    );
    let left_program = crate::global::role_program::role_program_from_image::<3>(
        &RoleProjection::<3, Global, 32>::IMAGE_REF,
    );
    let right_program = crate::global::role_program::role_program_from_image::<4>(
        &RoleProjection::<4, Global, 32>::IMAGE_REF,
    );
    let mailboxes = Mailboxes::new(&[0, 1, 2, 3, 4, 10], LANES).unwrap();
    let resource_path = mailboxes.directory().join("resource");
    let mut resource = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&resource_path)
        .unwrap();
    std::fs::remove_file(resource_path).unwrap();
    resource.write_all(&41u32.to_be_bytes()).unwrap();
    // SAFETY: the service exclusively owns this unlinked file. The binding
    // outlives every issued view, the child process and its private queues.
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
    // SAFETY: only async-signal-safe fcntl touches the inherited owned FDs.
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
        let side_io = NativeTransport::new(
            mailboxes.directory(),
            mailboxes.descriptors(&[3, 4]).unwrap(),
            None,
        )
        .unwrap();
        let mut owner_slab = vec![0; 64 * 1024];
        let mut service_slab = vec![0; 64 * 1024];
        let mut side_slab = vec![0; 64 * 1024];
        let mut owner_storage = SessionKitStorage::uninit();
        let mut service_storage = SessionKitStorage::uninit();
        let mut side_storage = SessionKitStorage::uninit();
        let owner_rv = owner_storage
            .init()
            .rendezvous(&mut owner_slab, owner_io)
            .unwrap();
        let service_rv = service_storage
            .init()
            .rendezvous(&mut service_slab, service_io)
            .unwrap();
        let side_rv = side_storage
            .init()
            .rendezvous(&mut side_slab, side_io)
            .unwrap();
        assert!(owner_rv.enter(SID, &db_program).is_err());
        let mut owner = owner_rv.enter(SID, &owner_program).unwrap();
        let mut service = service_rv.enter(SID, &service_program).unwrap();
        let db = service_rv.enter(SID, &db_program).unwrap();
        let mut left = side_rv.enter(SID, &left_program).unwrap();
        let mut right = side_rv.enter(SID, &right_program).unwrap();
        tokio::join!(
            async {
                service.send::<Loan>(db).await.unwrap();
                for expected in [44, 40] {
                    let branch = service.offer().await.unwrap();
                    let mut bytes = [0; 4];
                    resource.seek(SeekFrom::Start(0)).unwrap();
                    resource.read_exact(&mut bytes).unwrap();
                    let previous = u32::from_be_bytes(bytes);
                    let value = match branch.label() {
                        3 => previous + branch.recv::<Read>().await.unwrap(),
                        6 => previous - branch.recv::<Subtract>().await.unwrap(),
                        label => panic!("unexpected DB choice {label}"),
                    };
                    assert_eq!(value, expected);
                    resource.seek(SeekFrom::Start(0)).unwrap();
                    resource.write_all(&value.to_be_bytes()).unwrap();
                    service.send::<Value>(&value).await.unwrap();
                }
                let delta = service.recv::<AfterRead>().await.unwrap();
                let mut bytes = [0; 4];
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.read_exact(&mut bytes).unwrap();
                let value = u32::from_be_bytes(bytes) + delta;
                resource.seek(SeekFrom::Start(0)).unwrap();
                resource.write_all(&value.to_be_bytes()).unwrap();
                service.send::<AfterValue>(&value).await.unwrap();
                drop(service.recv::<Finished>().await.unwrap());
            },
            async {
                let mut db = owner.recv::<Loan>().await.unwrap();
                for _ in 0..2 {
                    owner.send::<Loan>(db).await.unwrap();
                    db = owner.recv::<Loan>().await.unwrap();
                }
                // Only the final borrowed continuation is used after exec's
                // address space has exited. This wait observes the test child;
                // it does not decide a protocol branch or advance an Endpoint.
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
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                for line in String::from_utf8(output.stdout).unwrap().lines() {
                    std::eprintln!("native composed worker: {line}");
                }
                db.send::<AfterRead>(&5).await.unwrap();
                assert_eq!(db.recv::<AfterValue>().await.unwrap(), 45);
                owner.send::<Finished>(db).await.unwrap();
            },
            async {
                for number in [10, 20] {
                    left.send::<Read>(&number).await.unwrap();
                    assert_eq!(left.recv::<Value>().await.unwrap(), number + 1);
                }
            },
            async {
                for number in [10, 20] {
                    assert_eq!(right.recv::<Read>().await.unwrap(), number);
                    right.send::<Value>(&(number + 1)).await.unwrap();
                }
            }
        );
        let mut bytes = [0; 4];
        resource.seek(SeekFrom::Start(0)).unwrap();
        resource.read_exact(&mut bytes).unwrap();
        assert_eq!(u32::from_be_bytes(bytes), 45);
    }
    drop(mailboxes);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process entrypoint exercised by the composed parent proof"]
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
            let fields: Vec<i32> = item
                .split(',')
                .map(|field| field.parse().unwrap())
                .collect();
            assert_eq!(fields.len(), 3);
            let role = u8::try_from(fields[0]).unwrap();
            let lane = u8::try_from(fields[1]).unwrap();
            assert!(fields[2] >= 0);
            // SAFETY: exec inherited these uniquely owned descriptors once.
            let socket = unsafe { UnixDatagram::from_raw_fd(fields[2]) };
            ((role, lane), socket)
        })
        .collect();
    let io = NativeTransport::new(&directory, descriptors, None).unwrap();
    let () = g::ProgramProjection::<Global, 32>::VALIDATION;
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 32>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 32>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let delta = Cell::new(3u32);
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, io).unwrap();
    rv.set_resolver(
        &db_program,
        ResolverRef::<702>::decision_state(&delta, |value| {
            Ok(if value.get() % 2 == 1 {
                DecisionArm::Left
            } else {
                DecisionArm::Right
            })
        }),
    )
    .unwrap();
    assert!(rv.enter(SID, &db_program).is_err());
    let mut worker = rv.enter(SID, &worker_program).unwrap();
    for (number, expected) in [(3, 44), (4, 40)] {
        delta.set(number);
        let mut db = worker.recv::<Loan>().await.unwrap();
        let branch = db.offer().await.unwrap();
        match branch.label() {
            3 => branch.send::<Read>(&number).await.unwrap(),
            6 => branch.send::<Subtract>(&number).await.unwrap(),
            label => panic!("unexpected resolver choice {label}"),
        }
        assert_eq!(db.recv::<Value>().await.unwrap(), expected);
        worker.send::<Loan>(db).await.unwrap();
    }
}
