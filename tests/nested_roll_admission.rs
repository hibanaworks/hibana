//! Drive source-model cases through public endpoints after every bounded prefix.
mod common;
use hibana::{
    g,
    runtime::{SessionKitStorage, ids::SessionId, program::project},
};

type Prefix = g::Msg<231, ()>;
type Work = g::Msg<232, ()>;
type Ack = g::Msg<233, ()>;
type Tick = g::Msg<234, ()>;

fn global() -> impl hibana::runtime::program::Projectable {
    g::seq(
        g::send::<0, 1, Prefix>(),
        g::par(
            g::seq(g::send::<0, 1, Work>(), g::send::<1, 0, Ack>()).roll(),
            g::send::<0, 2, Tick>(),
        )
        .roll(),
    )
    .roll()
}

#[derive(Clone, Copy, Debug)]
enum Letter {
    Prefix,
    Exchange,
    Tick,
}
impl Letter {
    fn decode(byte: u8) -> Self {
        match byte {
            0 => Self::Prefix,
            1 => Self::Exchange,
            2 => Self::Tick,
            _ => panic!("invalid source-model letter"),
        }
    }
}

#[test]
fn every_bounded_source_grouping_can_be_driven() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = global();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let input = include_bytes!("data/nested_roll_scopes.bin");
            assert_eq!(&input[..4], b"HNS\x01");
            let count = u32::from_be_bytes(input[4..8].try_into().unwrap());
            assert!(count > 0, "empty scoped source inventory");
            let mut rows = &input[8..];
            let mut attempts = 0;
            for case in 0..count {
                let (&length, rest) = rows.split_first().unwrap();
                let (prefix, rest) = rest.split_at(usize::from(length));
                let [probe, expected, rest @ ..] = rest else {
                    panic!("truncated source query");
                };
                assert!(*expected <= 1);
                let (groups, tail) = rest.split_at(2 * (prefix.len() + 1));
                rows = tail;
                let (groups, remainder) = groups.as_chunks::<2>();
                assert!(remainder.is_empty());
                let commands = prefix.iter().chain([probe]).zip(groups)
                    .map(|(letter, group)| (Letter::decode(*letter), u16::from_be_bytes(*group)))
                    .collect::<Vec<_>>();
                // A pending parallel scope statically borrows the parent. Try
                // its forbidden outer message on each child, on fresh runs.
                let forbidden_outer = *expected == 0 && matches!(Letter::decode(*probe), Letter::Prefix)
                    && commands.last().unwrap().1 != 0;
                for child in 0..if forbidden_outer { 2 } else { 1 } {
                    let network = common::TestTransport::new();
                    let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
                    let mut storage0 = SessionKitStorage::uninit();
                    let mut storage1 = SessionKitStorage::uninit();
                    let mut storage2 = SessionKitStorage::uninit();
                    let rv0 = storage0.init().rendezvous(&mut slab0, network.clone()).unwrap();
                    let rv1 = storage1.init().rendezvous(&mut slab1, network.clone()).unwrap();
                    let rv2 = storage2.init().rendezvous(&mut slab2, network).unwrap();
                    let sid = SessionId::new(9282);
                    let mut a = rv0.enter(sid, &p0).unwrap();
                    let mut b = rv1.enter(sid, &p1).unwrap();
                    let mut c = rv2.enter(sid, &p2).unwrap();
                    let execution = async {
                        let mut position = 0;
                        for chunk in commands.chunk_by(|left, right| left.1 == right.1) {
                            if chunk[0].1 == 0 {
                                for (letter, _) in chunk {
                                    if position == prefix.len() {
                                        let result = match letter {
                                            Letter::Prefix => a.send::<Prefix>(&()).await,
                                            Letter::Exchange => a.send::<Work>(&()).await,
                                            Letter::Tick => a.send::<Tick>(&()).await,
                                        };
                                        return Ok::<_, hibana::EndpointError>(result.is_ok());
                                    }
                                    assert!(matches!(letter, Letter::Prefix));
                                    tokio::try_join!(a.send::<Prefix>(&()), b.recv::<Prefix>())?;
                                    position += 1;
                                }
                            } else {
                                let [mut exchanges, mut ticks] = a.par()?;
                                let [mut server] = b.par()?;
                                let [mut ticker] = c.par()?;
                                for (letter, _) in chunk {
                                    if position == prefix.len() {
                                        let result = match letter {
                                            Letter::Prefix if child == 0 => exchanges.send::<Prefix>(&()).await,
                                            Letter::Prefix => ticks.send::<Prefix>(&()).await,
                                            Letter::Exchange => exchanges.send::<Work>(&()).await,
                                            Letter::Tick => ticks.send::<Tick>(&()).await,
                                        };
                                        return Ok(result.is_ok());
                                    }
                                    match letter {
                                        Letter::Prefix => panic!("outer prefix inside a source parallel group"),
                                        Letter::Exchange => {
                                            tokio::try_join!(
                                                async {
                                                    exchanges.send::<Work>(&()).await?;
                                                    exchanges.recv::<Ack>().await
                                                },
                                                async {
                                                    server.recv::<Work>().await?;
                                                    server.send::<Ack>(&()).await
                                                },
                                            )?;
                                        }
                                        Letter::Tick => {
                                            tokio::try_join!(ticks.send::<Tick>(&()), ticker.recv::<Tick>())?;
                                        }
                                    }
                                    position += 1;
                                }
                            }
                        }
                        panic!("source probe was not executed");
                    };
                    let result = tokio::time::timeout(core::time::Duration::from_millis(1), execution).await;
                    assert!(matches!(result, Ok(Ok(accepted)) if accepted == (*expected == 1)),
                        "case={case} commands={commands:?} child={child} result={result:?}");
                    attempts += 1;
                }
            }
            assert!(rows.is_empty(), "surplus source query bytes");
            println!("Nested-roll scoped admission: queries={count} attempts={attempts} runtime-only=0 model-only=0 unreachable=0");
        });
}

#[test]
fn flat_calls_never_admit_an_operation_outside_the_source_language() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let global = global();
            let p0 = project::<0>(&global);
            let p1 = project::<1>(&global);
            let p2 = project::<2>(&global);
            let input = include_bytes!("data/nested_roll_admission.bin");
            assert_eq!(&input[..4], b"HNR\x01");
            let count = u32::from_be_bytes(input[4..8].try_into().unwrap());
            assert!(count > 0, "empty source-model inventory");
            let mut rows = &input[8..];
            let (mut attempted, mut blocked_prefixes, mut runtime_only, mut other_source_cuts) = (0, 0, 0, 0);
            let mut observations = String::from("prefix\tprobe\texpected\toutcome\n");
            for _ in 0..count {
                let (&length, rest) = rows.split_first().expect("missing source-model case");
                assert!(length <= 7);
                let (prefix, rest) = rest
                    .split_at_checked(usize::from(length))
                    .expect("truncated source-model prefix");
                let [probe, expected, tail @ ..] = rest else {
                    panic!("missing source-model verdict");
                };
                rows = tail;
                let prefix = prefix
                    .iter()
                    .copied()
                    .map(Letter::decode)
                    .collect::<Vec<_>>();
                let probe = Letter::decode(*probe);
                let expected = match *expected {
                    0 => false,
                    1 => true,
                    _ => panic!("invalid source-model verdict"),
                };
                let network = common::TestTransport::new();
                let [mut slab0, mut slab1, mut slab2] = [[0; 32 * 1024]; 3];
                let mut storage0 = SessionKitStorage::uninit();
                let mut storage1 = SessionKitStorage::uninit();
                let mut storage2 = SessionKitStorage::uninit();
                let rv0 = storage0
                    .init()
                    .rendezvous(&mut slab0, network.clone())
                    .unwrap();
                let rv1 = storage1
                    .init()
                    .rendezvous(&mut slab1, network.clone())
                    .unwrap();
                let rv2 = storage2.init().rendezvous(&mut slab2, network).unwrap();
                let sid = SessionId::new(9281);
                let mut a = rv0.enter(sid, &p0).unwrap();
                let mut b = rv1.enter(sid, &p1).unwrap();
                let mut c = rv2.enter(sid, &p2).unwrap();
                let replay = async {
                  for letter in &prefix {
                    match letter {
                        Letter::Prefix => {
                            a.send::<Prefix>(&()).await?;
                            b.recv::<Prefix>().await?;
                        }
                        Letter::Exchange => {
                            a.send::<Work>(&()).await?;
                            b.recv::<Work>().await?;
                            b.send::<Ack>(&()).await?;
                            a.recv::<Ack>().await?;
                        }
                        Letter::Tick => {
                            a.send::<Tick>(&()).await?;
                            c.recv::<Tick>().await?;
                        }
                    }
                  }
                  Ok::<(),hibana::EndpointError>(())
                };
                let replay = tokio::time::timeout(core::time::Duration::from_millis(1),replay).await;
                if !matches!(replay,Ok(Ok(()))) {
                    blocked_prefixes += 1;
                    observations.push_str(&format!("{prefix:?}\t{probe:?}\t{expected}\tunreachable:{replay:?}\n"));
                    continue;
                }
                let outcome = match probe {
                    Letter::Prefix => {
                        tokio::time::timeout(
                            core::time::Duration::from_millis(1),
                            a.send::<Prefix>(&()),
                        )
                        .await
                    }
                    Letter::Exchange => {
                        tokio::time::timeout(
                            core::time::Duration::from_millis(1),
                            a.send::<Work>(&()),
                        )
                        .await
                    }
                    Letter::Tick => {
                        tokio::time::timeout(
                            core::time::Duration::from_millis(1),
                            a.send::<Tick>(&()),
                        )
                        .await
                    }
                };
                let accepted = matches!(outcome, Ok(Ok(())));
                attempted += 1;
                observations.push_str(&format!("{prefix:?}\t{probe:?}\t{expected}\t{outcome:?}\n"));
                if accepted != expected {
                    if accepted { runtime_only+=1; } else { other_source_cuts+=1; }
                }
            }
            assert!(rows.is_empty(), "unexpected source-model cases");
            let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nested-roll-admission");
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("observations.tsv"),observations).unwrap();
            println!("Nested-roll flat soundness: inventory={count} attempted={attempted} blocked-prefixes={blocked_prefixes} runtime-only={runtime_only} alternatives-at-other-cuts={other_source_cuts}");
            assert_eq!(attempted+blocked_prefixes,count);
            // A bare call chooses one repeat boundary. Existential admission
            // over all unfoldings is a soundness oracle for that call, not
            // an exact-next-operation oracle for its concrete source cut.
            // The scoped test above drives every distinct bounded grouping.
            assert_eq!(runtime_only, 0);
        });
}
