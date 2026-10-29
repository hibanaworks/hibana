use super::*;
use hibana::runtime::wire::{CodecError, Payload, ReceivedPayload, WireEncode, WirePayload};
use std::cell::Cell;

thread_local! {
    static DECODED: Cell<usize> = const { Cell::new(0) };
}

struct Request(u8);
impl WireEncode for Request {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.0.encode_into(out)
    }
}
impl WirePayload for Request {
    const SCHEMA_ID: u32 = 0x4400_0101;
    type Decoded<'a> = ReceivedPayload<'a, Self>;
    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        if matches!(input.as_bytes(), [1..=255]) {
            Ok(())
        } else {
            Err(CodecError::Malformed)
        }
    }
    fn decode_validated_payload<'a>(input: ReceivedPayload<'a, Self>) -> Self::Decoded<'a> {
        DECODED.with(|count| count.set(count.get() + 1));
        input
    }
}

#[test]
fn received_payloads_survive_direct_and_branch_receives_in_parallel() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            DECODED.with(|count| count.set(0));
            let global = g::par(
                g::seq(
                    g::send::<0, 1, Msg<1, Request>>(),
                    g::send::<1, 0, Msg<2, u8>>(),
                ),
                g::route(
                    g::send::<2, 1, Msg<3, Request>>(),
                    g::send::<2, 1, Msg<4, Request>>(),
                ),
            );
            let programs = (
                project::<0>(&global),
                project::<1>(&global),
                project::<2>(&global),
            );
            let mut slab = [0; 32768];

            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, TestTransport::new())
                .unwrap();
            let sid = SessionId::new(1);
            let mut sender = rv.enter(sid, &programs.0).unwrap();
            let mut receiver = rv.enter(sid, &programs.1).unwrap();
            let mut chooser = rv.enter(sid, &programs.2).unwrap();
            let [mut direct, mut choice] = receiver.par().unwrap();
            let ((), (), ()) = tokio::try_join!(
                async {
                    sender.send::<Msg<1, Request>>(&Request(17)).await?;
                    assert_eq!(sender.recv::<Msg<2, u8>>().await?, 17);
                    Ok::<_, hibana::EndpointError>(())
                },
                async { chooser.send::<Msg<3, Request>>(&Request(23)).await },
                async {
                    let answer = {
                        let (direct_payload, branch_payload) =
                            tokio::try_join!(direct.recv::<Msg<1, Request>>(), async {
                                let branch = choice.offer().await?;
                                assert_eq!(branch.label(), 3);
                                branch.recv::<Msg<3, Request>>().await
                            },)?;
                        assert_eq!(direct_payload.as_bytes(), &[17]);
                        assert_eq!(branch_payload.as_bytes(), &[23]);
                        direct_payload.as_bytes()[0]
                    };
                    direct.send::<Msg<2, u8>>(&answer).await?;
                    Ok(())
                }
            )
            .unwrap();
            DECODED.with(|count| assert_eq!(count.get(), 2));
            assert_eq!(
                core::mem::size_of::<ReceivedPayload<'_, Request>>(),
                core::mem::size_of::<Payload<'_>>()
            );
        });
}

#[test]
fn invalid_and_out_of_order_receives_never_decode_received_payloads() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for invalid_bytes in [true, false] {
                DECODED.with(|count| count.set(0));
                let global = g::seq(
                    g::send::<0, 1, Msg<1, Request>>(),
                    g::send::<0, 1, Msg<2, Request>>(),
                );
                let programs = (project::<0>(&global), project::<1>(&global));
                let mut slab = [0; 8192];

                let mut storage = SessionKitStorage::uninit();
                let rv = storage
                    .init()
                    .rendezvous(&mut slab, TestTransport::new())
                    .unwrap();
                let mut sender = rv.enter(SessionId::new(1), &programs.0).unwrap();
                let mut receiver = rv.enter(SessionId::new(1), &programs.1).unwrap();
                if invalid_bytes {
                    sender.send::<Msg<1, Request>>(&Request(0)).await.unwrap();
                    assert!(receiver.recv::<Msg<1, Request>>().await.is_err());
                } else {
                    assert!(receiver.recv::<Msg<2, Request>>().await.is_err());
                }
                DECODED.with(|count| assert_eq!(count.get(), 0));
            }
        });
}
