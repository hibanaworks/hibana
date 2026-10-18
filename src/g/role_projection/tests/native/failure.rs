//! Losing a remote holder ends the run, without restoring its operation right.
use super::*;
use crate::transport::TransportError;
use std::time::Instant;

type Global = g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<10, 2, Read>, g::Send<1, 0, Loan>>>;
const LANES: usize = g::ProgramProjection::<Global, 8>::SOURCE_EFF_LIST.len();
const SID: SessionId = SessionId::new(9061);
const BEFORE: &str = "g::role_projection::tests::native::failure::worker_exits_before_receipt";
const AFTER: &str = "g::role_projection::tests::native::failure::worker_exits_after_receipt";

#[tokio::test(flavor = "current_thread")]
async fn a_remote_holder_loss_is_observable_without_reissuing_its_endpoint() {
    let () = g::ProgramProjection::<Global, 8>::VALIDATION;
    let owner_program = crate::global::role_program::role_program_from_image::<0>(
        &RoleProjection::<0, Global, 8>::IMAGE_REF,
    );
    let service_program = crate::global::role_program::role_program_from_image::<2>(
        &RoleProjection::<2, Global, 8>::IMAGE_REF,
    );
    let db_program = crate::global::role_program::role_program_from_image::<10>(
        &RoleProjection::<10, Global, 8>::IMAGE_REF,
    );
    // Failure injection belongs to the test harness. Both cases execute the
    // same global and the same parent localsides with no protocol mode flag.
    for worker_test in [BEFORE, AFTER] {
        let mailboxes = Mailboxes::new(&[0, 1, 2, 10], LANES).unwrap();
        // SAFETY: this namespace has one initial issuer, retained until every
        // child and runtime is gone and the entire native namespace is fenced.
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
            .args(["--ignored", "--exact", worker_test, "--nocapture"])
            .env("HIBANA_NATIVE_DIRECTORY", mailboxes.directory())
            .env("HIBANA_NATIVE_DESCRIPTORS", descriptors)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: only async-signal-safe fcntl runs in the post-fork callback.
        // These owned descriptors alone have close-on-exec cleared.
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
            let io = NativeTransport::new(
                mailboxes.directory(),
                mailboxes.descriptors(&[0, 2, 10]).unwrap(),
                Some((10, &binding)),
            )
            .unwrap();
            let mut slab = vec![0; 64 * 1024];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage.init().rendezvous(&mut slab, io).unwrap();
            let mut owner = rv.enter(SID, &owner_program).unwrap();
            let mut service = rv.enter(SID, &service_program).unwrap();
            let db = rv.enter(SID, &db_program).unwrap();
            let began = Instant::now();
            let (returned, read) = tokio::time::timeout(Duration::from_secs(8), async {
                tokio::join!(
                    async {
                        owner.send::<Loan>(db).await.unwrap();
                        owner.recv::<Loan>().await
                    },
                    service.recv::<Read>(),
                )
            })
            .await
            .expect("the carrier deadline must end the run before the test watchdog");
            assert!(returned.is_err(), "the lost view cannot be returned");
            assert!(
                read.is_err(),
                "the lost view cannot perform a service request"
            );
            assert!(began.elapsed() >= Duration::from_secs(2));
            let errors = [
                std::format!("{:?}", returned.err().expect("lost view")),
                std::format!("{:?}", read.expect_err("lost request")),
            ];
            let deadline = std::format!("Transport({:?})", TransportError::Deadline);
            assert!(
                errors.iter().any(|error| error.contains(&deadline)),
                "{errors:?}"
            );
            assert!(
                errors
                    .iter()
                    .any(|error| error.contains("SessionFault(TransportClosed)")),
                "{errors:?}"
            );
            // Both roles retain the first fault. Neither a later operation nor
            // attachment under another SessionId repairs the run.
            let repeated = [
                std::format!(
                    "{:?}",
                    owner.recv::<Loan>().await.err().expect("faulted owner")
                ),
                std::format!(
                    "{:?}",
                    service.recv::<Read>().await.expect_err("faulted service")
                ),
            ];
            assert!(
                repeated
                    .iter()
                    .all(|error| error.contains("SessionFault(TransportClosed)"))
            );
            assert!(rv.enter(SID, &db_program).is_err());
            assert!(
                rv.enter(SessionId::new(SID.raw() + 1), &db_program)
                    .is_err()
            );
            tokio::time::timeout(Duration::from_secs(3), async {
                while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .expect("the failed worker must exit before its descriptors are fenced");
            let output = child.0.take().unwrap().wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "native child failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            // Observe whether the accepted owned frame remained in its actual
            // OS queue. MSG_PEEK never consumes or supplies protocol evidence.
            let mut queued = 0;
            for (_, socket) in mailboxes.descriptors(&[1]).unwrap() {
                let mut bytes = [0; 32];
                // SAFETY: the live nonblocking socket and writable bytes last
                // through this syscall. MSG_PEEK cannot consume the datagram.
                let length = unsafe {
                    libc::recv(
                        socket.as_raw_fd(),
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                        libc::MSG_PEEK,
                    )
                };
                if length < 0 {
                    assert_eq!(
                        std::io::Error::last_os_error().kind(),
                        std::io::ErrorKind::WouldBlock
                    );
                    continue;
                }
                assert_eq!(length, 11);
                assert_eq!(bytes[..4], SID.raw().to_be_bytes());
                assert_eq!(bytes[5..7], [0, 1]);
                assert_eq!(bytes[8], 10);
                queued += 1;
            }
            match worker_test {
                BEFORE => assert_eq!(queued, 1),
                AFTER => assert_eq!(queued, 0),
                _ => unreachable!(),
            }
            std::eprintln!(
                "native terminal boundary worker={worker_test} accepted_frames_left={queued} \
                 errors={errors:?} elapsed_ms={}",
                began.elapsed().as_millis(),
            );
        }
        // Fence accepted but unread data only after all old runtimes and the
        // child are gone; no new run can receive this namespace's old frames.
        drop(mailboxes);
    }
}

#[test]
#[ignore = "separate process failure point exercised by the parent proof"]
fn worker_exits_before_receipt() {}

#[tokio::test(flavor = "current_thread")]
#[ignore = "separate process failure point exercised by the parent proof"]
async fn worker_exits_after_receipt() {
    let directory = std::path::PathBuf::from(std::env::var_os("HIBANA_NATIVE_DIRECTORY").unwrap());
    let descriptors: Vec<_> = std::env::var("HIBANA_NATIVE_DESCRIPTORS")
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
            // SAFETY: each descriptor was uniquely passed through exec, and
            // no other object in this process owns it.
            let socket = unsafe { UnixDatagram::from_raw_fd(fields[2]) };
            ((role, lane), socket)
        })
        .collect();
    let io = NativeTransport::new(&directory, descriptors, None).unwrap();
    let () = g::ProgramProjection::<Global, 8>::VALIDATION;
    let worker_program = crate::global::role_program::role_program_from_image::<1>(
        &RoleProjection::<1, Global, 8>::IMAGE_REF,
    );
    let mut slab = vec![0; 64 * 1024];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, io).unwrap();
    let mut worker = rv.enter(SID, &worker_program).unwrap();
    drop(worker.recv::<Loan>().await.unwrap());
}
