//! Byte-level counterexamples for the source/local dependency boundary.
use super::*;
use crate::global::role_program::{ROLE_IMAGE_DEPENDENCY_STRIDE, ROLE_IMAGE_EVENT_STRIDE};

type Material = g::Msg<1, Endpoint<'static, 36>>;
type Started = g::Msg<161, ()>;
type Admitted = g::Msg<165, ()>;
type Independent = g::Msg<2, ()>;
type Request = g::Msg<6, u64>;
type More = g::Msg<7, u64>;
type Finished = g::Msg<8, u64>;
type Rejected = g::Msg<217, u64>;
type Interrupted = g::Msg<219, u64>;
type Stop = g::Msg<9, ()>;
type Stopped = g::Msg<10, ()>;
type Notice = g::Msg<3, ()>;
type ParallelReplies = g::Seq<
    g::Seq<g::Send<50, 51, Material>, g::Send<51, 50, Material>>,
    g::Seq<
        g::Seq<g::Send<1, 10, Started>, g::Send<10, 13, Admitted>>,
        g::Par<
            g::Par<g::Send<8, 9, Independent>, g::Send<9, 27, Independent>>,
            g::Par<
                g::Seq<
                    g::Roll<
                        g::Route<
                            g::Seq<
                                g::Send<10, 11, Request>,
                                g::Route<
                                    g::Send<11, 10, More>,
                                    g::Route<
                                        g::Send<11, 10, Finished>,
                                        g::Route<
                                            g::Send<11, 10, Rejected>,
                                            g::Send<11, 10, Interrupted>,
                                        >,
                                    >,
                                >,
                            >,
                            g::Send<10, 11, Stop>,
                        >,
                    >,
                    g::Send<11, 10, Stopped>,
                >,
                g::Send<11, 28, Notice>,
            >,
        >,
    >,
>;

#[test]
fn nested_parallel_replies_preserve_the_prefix_in_every_alternative() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            use crate::{
                runtime::{SessionKitStorage, ids::SessionId, program::project},
                test_support::transport::TestTransport,
            };
            let global = g::Program::<ParallelReplies>::new();
            let owner_program = project::<50>(&global);
            let worker_program = project::<51>(&global);
            let material_program = project::<36>(&global);
            let start_program = project::<1>(&global);
            let client_program = project::<10>(&global);
            let admission_program = project::<13>(&global);
            let first_program = project::<8>(&global);
            let middle_program = project::<9>(&global);
            let last_program = project::<27>(&global);
            let service_program = project::<11>(&global);
            let notice_program = project::<28>(&global);
            for number in 0..=4 {
                let network = TestTransport::new();
                let mut slab = [0; 128 * 1024];
                let mut storage = SessionKitStorage::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, network.clone())
                    .unwrap();
                let session = SessionId::new(9950 + number as u32);
                let mut owner = rv.enter(session, &owner_program).unwrap();
                let material = rv.enter(session, &material_program).unwrap();
                let mut worker = rv.enter(session, &worker_program).unwrap();
                let mut client = rv.enter(session, &client_program).unwrap();
                if number == 0 {
                    assert!(
                        tokio::time::timeout(
                            core::time::Duration::from_secs(1),
                            client.send::<Stop>(&())
                        )
                        .await
                        .expect("an early stop must be rejected without waiting")
                        .is_err()
                    );
                    assert!(network.queue_is_empty());
                    continue;
                }
                let mut starter = rv.enter(session, &start_program).unwrap();
                let mut admission = rv.enter(session, &admission_program).unwrap();
                let mut first = rv.enter(session, &first_program).unwrap();
                let mut middle = rv.enter(session, &middle_program).unwrap();
                let mut last = rv.enter(session, &last_program).unwrap();
                let mut service = rv.enter(session, &service_program).unwrap();
                let mut notice = rv.enter(session, &notice_program).unwrap();
                tokio::time::timeout(core::time::Duration::from_secs(10), async {
                    tokio::join!(
                        async {
                            owner.send::<Material>(material).await.unwrap();
                            drop(owner.recv::<Material>().await.unwrap());
                        },
                        async {
                            let material = worker.recv::<Material>().await.unwrap();
                            worker.send::<Material>(material).await.unwrap();
                        }
                    );
                    let [mut incoming, mut outgoing] = middle.par().unwrap();
                    let [mut packets, mut notices] = service.par().unwrap();
                    tokio::join!(
                        async {
                            starter.send::<Started>(&()).await.unwrap();
                        },
                        async {
                            client.recv::<Started>().await.unwrap();
                            client.send::<Admitted>(&()).await.unwrap();
                            client.send::<Request>(&number).await.unwrap();
                            let reply = client.offer().await.unwrap();
                            let value = match reply.label() {
                                7 => reply.recv::<More>().await.unwrap(),
                                8 => reply.recv::<Finished>().await.unwrap(),
                                217 => reply.recv::<Rejected>().await.unwrap(),
                                219 => reply.recv::<Interrupted>().await.unwrap(),
                                _ => unreachable!(),
                            };
                            assert_eq!(value, number);
                            client.send::<Stop>(&()).await.unwrap();
                            client.recv::<Stopped>().await.unwrap();
                        },
                        async {
                            admission.recv::<Admitted>().await.unwrap();
                        },
                        async {
                            first.send::<Independent>(&()).await.unwrap();
                        },
                        async {
                            incoming.recv::<Independent>().await.unwrap();
                        },
                        async {
                            outgoing.send::<Independent>(&()).await.unwrap();
                        },
                        async {
                            last.recv::<Independent>().await.unwrap();
                        },
                        async {
                            let value = packets
                                .offer()
                                .await
                                .unwrap()
                                .recv::<Request>()
                                .await
                                .unwrap();
                            match value {
                                1 => packets.send::<More>(&value).await.unwrap(),
                                2 => packets.send::<Finished>(&value).await.unwrap(),
                                3 => packets.send::<Rejected>(&value).await.unwrap(),
                                4 => packets.send::<Interrupted>(&value).await.unwrap(),
                                _ => unreachable!(),
                            }
                            packets.offer().await.unwrap().recv::<Stop>().await.unwrap();
                            packets.send::<Stopped>(&()).await.unwrap();
                        },
                        async {
                            notices.send::<Notice>(&()).await.unwrap();
                        },
                        async {
                            notice.recv::<Notice>().await.unwrap();
                        },
                    );
                })
                .await
                .expect("independent replies and their continuation must finish");
                assert!(network.queue_is_empty());
            }
        });
}

#[test]
fn source_order_rejects_a_missing_join_and_spurious_parallel_order() {
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<
            g::Par<g::Send<1, 2, Read>, g::Send<10, 3, Read>>,
            g::Seq<g::Send<1, 2, Value>, g::Send<1, 0, Loan>>,
        >,
    >;
    const N: usize = RoleProjection::<1, Steps, 16>::IMAGE_REF
        .columns()
        .blob_len();
    const BYTES: [[u8; N]; 2] = {
        let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
        let descriptor = RoleDescriptorRef::from_resident(original);
        let columns = original.columns();
        let mut bytes = [0; N];
        let mut offset = 0;
        while offset < N {
            bytes[offset] = descriptor.proof_byte_at(offset);
            offset += 1;
        }
        let mut worker_read = None;
        let mut db_read = None;
        let mut after_join = None;
        let mut row = 0;
        while row < original.local_step_count() {
            match original.local_step_node(row).unwrap().action() {
                LocalAction::Send {
                    label: 3, peer: 2, ..
                } => worker_read = Some(row),
                LocalAction::Send {
                    label: 3, peer: 3, ..
                } => db_read = Some(row),
                LocalAction::Send {
                    label: 4, peer: 2, ..
                } => after_join = Some(row),
                LocalAction::Send { .. }
                | LocalAction::Recv { .. }
                | LocalAction::Local { .. }
                | LocalAction::Terminate => (),
            }
            row += 1;
        }
        let worker_read = worker_read.unwrap();
        let db_read = db_read.unwrap();
        let after_join = after_join.unwrap();
        assert!(worker_read < db_read && db_read < after_join);
        let join_reference =
            columns.events.offset as usize + after_join * ROLE_IMAGE_EVENT_STRIDE + 2;
        let mut missing_join = bytes;
        missing_join[join_reference] = 255;
        missing_join[join_reference + 1] = 255;
        let db_reference = columns.events.offset as usize + db_read * ROLE_IMAGE_EVENT_STRIDE + 2;
        let dependency = u16::from_le_bytes([bytes[db_reference], bytes[db_reference + 1]]);
        assert!(dependency != u16::MAX);
        let end = columns.dependencies.offset as usize
            + dependency as usize * ROLE_IMAGE_DEPENDENCY_STRIDE
            + 2;
        let mut extra_order = bytes;
        let encoded = ((worker_read + 1) as u16).to_le_bytes();
        extra_order[end] = encoded[0];
        extra_order[end + 1] = encoded[1];
        [missing_join, extra_order]
    };
    const IMAGES: [RoleImageRef; 2] = {
        let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
        [
            RoleImageRef::new(
                original.program(),
                original.role(),
                original.facts(),
                original.columns(),
                &BYTES[0],
            ),
            RoleImageRef::new(
                original.program(),
                original.role(),
                original.facts(),
                original.columns(),
                &BYTES[1],
            ),
        ]
    };
    let original = &RoleProjection::<1, Steps, 16>::IMAGE_REF;
    assert!(crate::g::role_projection::source_order::check::<Steps, 16>(
        original
    ));
    // Structural image validity alone cannot prove source dependency coverage.
    // The typed projection additionally runs source_order before publication.
    let missing_join = &IMAGES[0];
    let extra_order = &IMAGES[1];
    assert!(!crate::g::role_projection::source_order::check::<Steps, 16>(missing_join));

    assert!(!crate::g::role_projection::source_order::check::<Steps, 16>(extra_order));
    #[cfg(not(miri))]
    {
        use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
        let descriptor = RoleDescriptorRef::from_resident(original);
        let replies = crate::runtime::program::project::<10>(&g::Program::<ParallelReplies>::new());
        let source = std::format!(
            "import Hibana\n\nset_option maxRecDepth 10000\n\n\
             def sourceOrderChoreo : Hibana.Choreo := {}\n\n\
             def sourceOrderValid : Hibana.RustDescriptorImage := {}\n\n\
             def sourceOrderMissingJoin : Hibana.RustDescriptorImage := {}\n\n\
             def sourceOrderExtraOrder : Hibana.RustDescriptorImage := {}\n\n\
             theorem sourceOrderValidAccepted :\n  \
             sourceOrderValid.actorOrderCheck sourceOrderChoreo = true := by decide\n\n\
             theorem sourceOrderMissingJoinRejected :\n  \
             sourceOrderMissingJoin.actorRowsCheck sourceOrderChoreo = true ∧\n  \
             sourceOrderMissingJoin.actorOrderCheck sourceOrderChoreo = false := by decide\n\n\
             theorem sourceOrderExtraOrderRejected :\n  \
             sourceOrderExtraOrder.actorRowsCheck sourceOrderChoreo = true ∧\n  \
             sourceOrderExtraOrder.actorOrderCheck sourceOrderChoreo = false := by decide\n\n\
             def parallelReplyChoreo : Hibana.Choreo := {}\n\n\
             def parallelReplyImage : Hibana.RustDescriptorImage := {}\n\n\
             theorem parallelReplyImagePreservesSourceOrder :\n  \
             parallelReplyImage.actorOrderCheck parallelReplyChoreo = true := by decide\n",
            Steps::lean_source(),
            descriptor_source(descriptor),
            descriptor_source(RoleDescriptorRef::from_resident(missing_join)),
            descriptor_source(RoleDescriptorRef::from_resident(extra_order)),
            ParallelReplies::lean_source(),
            descriptor_source(RoleDescriptorRef::from_resident(replies.role_image_ref())),
        ) + &root_sequence_certificate();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("SourceOrderGenerated.lean"), source).unwrap();
    }
}

#[cfg(not(miri))]
fn root_sequence_certificate() -> std::string::String {
    use crate::test_support::{lean_choreo::LeanChoreo, lean_descriptor::descriptor_source};
    type Loan = g::Msg<30, Endpoint<'static, 3>>;
    type First = g::Msg<31, ()>;
    type Second = g::Msg<32, ()>;
    type Steps = g::Seq<
        g::Send<0, 1, Loan>,
        g::Seq<g::Send<3, 2, First>, g::Seq<g::Send<1, 2, Second>, g::Send<1, 0, Loan>>>,
    >;
    const N: usize = RoleProjection::<1, Steps, 8>::IMAGE_REF
        .columns()
        .blob_len();
    let global = g::Program::<Steps>::new();
    let projected = crate::runtime::program::project::<1>(&global);
    let original = projected.role_image_ref();
    assert!(crate::g::role_projection::source_order::check::<Steps, 8>(
        original
    ));
    let row = (0..original.local_step_count())
        .find(|&row| {
            matches!(
                original.local_step_node(row).unwrap().action(),
                LocalAction::Send { label: 32, .. }
            )
        })
        .unwrap();
    let dependency = original.dependency_for_index(row).unwrap();
    assert!(dependency.scope().is_none());
    assert!(dependency.start() < dependency.end() && dependency.end() <= row);
    let descriptor = RoleDescriptorRef::from_resident(original);
    assert_eq!(original.columns().blob_len(), N);
    let mut bytes = [0; N];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        *byte = descriptor.proof_byte_at(offset);
    }
    let reference = original.columns().events.offset as usize + row * ROLE_IMAGE_EVENT_STRIDE + 2;
    bytes[reference..reference + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    let missing = std::boxed::Box::leak(std::boxed::Box::new(RoleImageRef::new(
        original.program(),
        original.role(),
        original.facts(),
        original.columns(),
        std::boxed::Box::leak(std::boxed::Box::new(bytes)),
    )));
    assert!(!crate::g::role_projection::source_order::check::<Steps, 8>(
        missing
    ));
    std::format!(
        "\ndef rootSequenceChoreo : Hibana.Choreo := {}\n\n\
         def rootSequenceImage : Hibana.RustDescriptorImage := {}\n\n\
         def rootSequenceMissing : Hibana.RustDescriptorImage := {}\n\n\
         theorem rootSequenceImagePreservesSourceOrder :\n  \
         rootSequenceImage.actorOrderCheck rootSequenceChoreo = true := by decide\n\n\
         theorem rootSequenceMissingDependencyRejected :\n  \
         rootSequenceMissing.actorRowsCheck rootSequenceChoreo = true ∧\n  \
         rootSequenceMissing.actorOrderCheck rootSequenceChoreo = false := by decide\n",
        Steps::lean_source(),
        descriptor_source(descriptor),
        descriptor_source(RoleDescriptorRef::from_resident(missing)),
    )
}
