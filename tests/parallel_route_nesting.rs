mod common;
#[path = "../examples/support/in_memory.rs"]
mod in_memory;
#[path = "../examples/support/in_process.rs"]
pub mod in_process;

use common::TestTransport;
use hibana::g::{self, Msg};
use hibana::runtime::program::{RoleProgram, project};
use hibana::runtime::{SessionKitStorage, ids::SessionId};

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn an_outer_choice_can_leave_a_repeated_arm_after_its_receives_converge()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};

    let global = g::route(
        g::seq(
            g::send::<0, 1, Msg<0, ()>>(),
            g::seq(g::send::<0, 2, Msg<1, ()>>(), g::send::<1, 2, Msg<2, ()>>()),
        ),
        g::seq(
            g::send::<0, 1, Msg<3, ()>>(),
            g::seq(g::send::<0, 2, Msg<4, ()>>(), g::send::<1, 2, Msg<5, ()>>()),
        )
        .roll(),
    )
    .resolve::<0>()
    .roll();
    let (p0, p1, p2) = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
    );
    for choices in [[false, false], [false, true], [true, false], [true, true]] {
        let network = in_memory::InMemoryTransport::new([]);
        let choice = core::cell::Cell::new(DecisionArm::Left);
        let [mut slab0, mut slab1, mut slab2] = [(); 3].map(|_| vec![0; 32 * 1024]);
        let (mut s0, mut s1, mut s2) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, network).unwrap();
        rv0.set_resolver(
            &p0,
            ResolverRef::<0>::decision_state(&choice, |choice| Ok(choice.get())),
        )
        .unwrap();
        let sid = SessionId::new(95);
        let mut controller = rv0.enter(sid, &p0).unwrap();
        let mut relay = rv1.enter(sid, &p1).unwrap();
        let mut receiver = rv2.enter(sid, &p2).unwrap();
        {
            (async {
                    async {

                        async { let _: () = (async {
                            crate::in_process::sleep(core::time::Duration::from_millis(1)).await;
                            for right in choices {
                                choice.set(if right {
                                    DecisionArm::Right
                                } else {
                                    DecisionArm::Left
                                });
                                for _ in 0..2 {
                                    let branch = controller.offer().await.unwrap_or_else(|error| {
                                        panic!("controller choices={choices:?} right={right}: {error:?}")
                                    });
                                    assert_eq!(branch.label(), if right { 3 } else { 0 });
                                    drop(branch);
                                }
                                if right {
                                    for _ in 0..2 {
                                        controller.send::<Msg<3, ()>>(&()).await.unwrap();
                                        controller.send::<Msg<4, ()>>(&()).await.unwrap();
                                    }
                                } else {
                                    controller.send::<Msg<0, ()>>(&()).await.unwrap();
                                    controller.send::<Msg<1, ()>>(&()).await.unwrap();
                                }
                            }
                        }).await;
                        Ok::<_, hibana::EndpointError>(()) };

                        async { {
                            for right in choices {
                                for _ in 0..2 {
                                    let branch = relay.offer().await.expect("relay preview");
                                    assert_eq!(branch.label(), if right { 3 } else { 0 });
                                    drop(branch);
                                }
                                if right {
                                    for _ in 0..2 {
                                        relay.recv::<Msg<3, ()>>().await.unwrap();
                                        relay.send::<Msg<5, ()>>(&()).await.unwrap();
                                    }
                                } else {
                                    relay.recv::<Msg<0, ()>>().await.unwrap();
                                    relay.send::<Msg<2, ()>>(&()).await.unwrap();
                                }
                            }
                        }
                        Ok::<_, hibana::EndpointError>(()) };

                        async { {
                            for right in choices {
                                for _ in 0..2 {
                                    let branch = receiver.offer().await.expect("receiver preview");
                                    assert_eq!(branch.label(), if right { 4 } else { 1 });
                                    drop(branch);
                                }
                                if right {
                                    for _ in 0..2 {
                                        receiver.recv::<Msg<4, ()>>().await.unwrap();
                                        receiver.recv::<Msg<5, ()>>().await.unwrap();
                                    }
                                } else {
                                    receiver.recv::<Msg<1, ()>>().await.unwrap();
                                    receiver.recv::<Msg<2, ()>>().await.unwrap();
                                }
                            }
                        }
                        Ok::<_, hibana::EndpointError>(()) };
}.await.expect("all child results are data");
                }).await;
        };
    }
    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_nested_resolver_can_select_another_arm_on_the_next_visit()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};

    let global = g::route(
        g::seq(g::send::<1, 0, Msg<0, ()>>(), g::send::<0, 1, Msg<1, ()>>()),
        g::route(
            g::seq(g::send::<1, 0, Msg<2, ()>>(), g::send::<0, 1, Msg<3, ()>>()),
            g::seq(g::send::<1, 0, Msg<4, ()>>(), g::send::<0, 1, Msg<5, ()>>()),
        )
        .resolve::<1>(),
    )
    .resolve::<0>()
    .roll();
    let (p0, p1) = (project::<0>(&global), project::<1>(&global));
    let network = in_memory::InMemoryTransport::new([]);
    let input = core::cell::Cell::new(DecisionArm::Left);
    let [mut slab0, mut slab1] = [vec![0; 32 * 1024], vec![0; 32 * 1024]];
    let (mut s0, mut s1) = (SessionKitStorage::uninit(), SessionKitStorage::uninit());
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    rv1.set_resolver(
        &p1,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    rv1.set_resolver(
        &p1,
        ResolverRef::<1>::decision_state(&input, |input| Ok(input.get())),
    )
    .unwrap();
    let sid = SessionId::new(94);
    let mut observer = rv0.enter(sid, &p0).unwrap();
    let mut controller = rv1.enter(sid, &p1).unwrap();

    {
        (async {
            async {
                async {
                    {
                        for choice in [DecisionArm::Left, DecisionArm::Right] {
                            input.set(choice);
                            let branch = controller.offer().await.unwrap();
                            drop(branch);
                            let branch = controller.offer().await.unwrap();
                            match choice {
                                DecisionArm::Left => {
                                    assert_eq!(branch.label(), 2);
                                    branch.send::<Msg<2, ()>>(&()).await.unwrap();
                                    controller.recv::<Msg<3, ()>>().await.unwrap();
                                }
                                DecisionArm::Right => {
                                    assert_eq!(branch.label(), 4);
                                    branch.send::<Msg<4, ()>>(&()).await.unwrap();
                                    controller.recv::<Msg<5, ()>>().await.unwrap();
                                }
                            }
                        }
                    }
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    {
                        for label in [2, 4] {
                            let branch = observer.offer().await.unwrap();
                            assert_eq!(branch.label(), label);
                            drop(branch);
                            let branch = observer.offer().await.unwrap();
                            match label {
                                2 => {
                                    branch.recv::<Msg<2, ()>>().await.unwrap();
                                    observer.send::<Msg<3, ()>>(&()).await.unwrap();
                                }
                                4 => {
                                    branch.recv::<Msg<4, ()>>().await.unwrap();
                                    observer.send::<Msg<5, ()>>(&()).await.unwrap();
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_nested_loan_keeps_its_owner_while_an_independent_parent_part_restarts()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::{Endpoint, runtime::transport::EndpointBinding};
    type Start = Msg<0, ()>;
    type Ack = Msg<1, ()>;
    type Loan<'r> = Msg<2, Endpoint<'r, 3>>;
    type Read = Msg<3, ()>;
    type Value = Msg<4, ()>;
    type Return<'r> = Msg<5, Endpoint<'r, 3>>;

    let global = g::par(
        g::seq(g::send::<0, 1, Start>(), g::send::<1, 0, Ack>()),
        g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(
                g::send::<3, 0, Read>(),
                g::seq(g::send::<0, 3, Value>(), g::send::<1, 0, Return>()),
            ),
        )
        .roll(),
    )
    .roll();
    let (p0, p1, p3) = (
        project::<0>(&global),
        project::<1>(&global),
        project::<3>(&global),
    );
    let sid = SessionId::new(93);
    // SAFETY: the test owns one resource and its only initial binding.
    let network =
        in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
    let [mut slab0, mut slab1] = [vec![0; 64 * 1024], vec![0; 64 * 1024]];
    let (mut s0, mut s1) = (SessionKitStorage::uninit(), SessionKitStorage::uninit());
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let initial_db = rv0.enter(sid, &p3).unwrap();

    {
        (async {
            async {
                async {
                    let _: () = (async {
                        let mut db = initial_db;
                        let [mut ordinary, mut loan] = owner.par().unwrap();
                        for _ in 0..2 {
                            let (returned, ()) = async {
                                async {
                                    Ok::<_, hibana::EndpointError>(
                                        (async {
                                            for _ in 0..2 {
                                                loan.send::<Loan>(db).await.unwrap();
                                                loan.recv::<Read>().await.unwrap();
                                                loan.send::<Value>(&()).await.unwrap();
                                                db = loan.recv::<Return>().await.unwrap();
                                            }
                                            db
                                        })
                                        .await,
                                    )
                                };

                                async {
                                    {
                                        hibana_runtime::yield_now().await;
                                        ordinary.send::<Start>(&()).await.unwrap();
                                        ordinary.recv::<Ack>().await.unwrap();
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };
                            }
                            .await
                            .expect("all child results are data");
                            db = returned;
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let [mut ordinary, mut loan] = worker.par().unwrap();
                        for _ in 0..2 {
                            async {
                                async {
                                    {
                                        for _ in 0..2 {
                                            let mut db = loan.recv::<Loan>().await.unwrap();
                                            db.send::<Read>(&()).await.unwrap();
                                            db.recv::<Value>().await.unwrap();
                                            loan.send::<Return>(db).await.unwrap();
                                        }
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };

                                async {
                                    {
                                        ordinary.recv::<Start>().await.unwrap();
                                        ordinary.send::<Ack>(&()).await.unwrap();
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };
                            }
                            .await
                            .expect("all child results are data");
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn retained_parallel_borrows_allow_a_nested_repeat()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::{Endpoint, runtime::transport::EndpointBinding};
    type Loan<'r> = Msg<0, Endpoint<'r, 3>>;
    type Read = Msg<1, ()>;
    type Value = Msg<2, ()>;
    type Return<'r> = Msg<3, Endpoint<'r, 3>>;
    type Start = Msg<4, ()>;
    type Work = Msg<5, ()>;
    type Done = Msg<6, ()>;
    type Ack = Msg<7, ()>;

    let global = g::par(
        g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(
                g::send::<3, 2, Read>(),
                g::seq(g::send::<2, 3, Value>(), g::send::<1, 0, Return>()),
            ),
        ),
        g::seq(
            g::send::<0, 1, Start>(),
            g::seq(
                g::send::<1, 2, Work>(),
                g::seq(g::send::<2, 1, Done>(), g::send::<1, 0, Ack>()),
            ),
        )
        .roll(),
    )
    .roll();
    let (p0, p1, p2, p3) = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
        project::<3>(&global),
    );
    let sid = SessionId::new(92);
    // SAFETY: the test owns one resource and its only initial binding.
    let network =
        in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
    let [mut slab0, mut slab1, mut slab2] =
        [vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]];
    let (mut s0, mut s1, mut s2) = (
        SessionKitStorage::uninit(),
        SessionKitStorage::uninit(),
        SessionKitStorage::uninit(),
    );
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network).unwrap();
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut service = rv2.enter(sid, &p2).unwrap();
    let initial_db = rv0.enter(sid, &p3).unwrap();

    {
        (async {
            async {
                async {
                    let _: () = (async {
                        let mut db = initial_db;
                        let [mut loan, mut work] = owner.par().unwrap();
                        for visit in 0..2 {
                            let (returned, ()) =
                                async {
                                    async {
                                        Ok::<_, hibana::EndpointError>(
                                            (async {
                                                loan.send::<Loan>(db).await.unwrap_or_else(|error| {
                                        panic!("owner loan in outer visit {visit}: {error:?}")
                                    });
                                                loan.recv::<Return>().await.expect("owner return")
                                            })
                                            .await,
                                        )
                                    };

                                    async {
                                        let _: () = (async {
                                        for inner in 0..2 {
                                            work.send::<Start>(&()).await.unwrap_or_else(|error| {
                                        panic!("owner start in visit {visit}/{inner}: {error:?}")
                                    });
                                            work.recv::<Ack>().await.expect("owner ack");
                                        }
                                    })
                                    .await;
                                        Ok::<_, hibana::EndpointError>(())
                                    };
                                }
                                .await
                                .expect("all child results are data");
                            db = returned;
                        }
                        drop(db);
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let [mut loan, mut work] = worker.par().unwrap();
                        for visit in 0..2 {
                            async {
                                async {
                                    let _: () = (async {
                                        let mut db =
                                            loan.recv::<Loan>().await.unwrap_or_else(|error| {
                                                panic!(
                                                    "worker loan in outer visit {visit}: {error:?}"
                                                )
                                            });
                                        db.send::<Read>(&()).await.expect("worker read");
                                        db.recv::<Value>().await.expect("worker value");
                                        loan.send::<Return>(db).await.unwrap_or_else(|error| {
                                            panic!(
                                                "worker return in outer visit {visit}: {error:?}"
                                            )
                                        });
                                    })
                                    .await;
                                    Ok::<_, hibana::EndpointError>(())
                                };

                                async {
                                    {
                                        for _ in 0..2 {
                                            work.recv::<Start>().await.expect("worker start");
                                            work.send::<Work>(&()).await.expect("worker work");
                                            work.recv::<Done>().await.expect("worker done");
                                            work.send::<Ack>(&()).await.expect("worker ack");
                                        }
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };
                            }
                            .await
                            .expect("all child results are data");
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let [mut db, mut work] = service.par().unwrap();
                        for _ in 0..2 {
                            async {
                                async {
                                    {
                                        db.recv::<Read>().await.expect("service read");
                                        db.send::<Value>(&()).await.expect("service value");
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };

                                async {
                                    {
                                        for _ in 0..2 {
                                            work.recv::<Work>().await.expect("service work");
                                            work.send::<Done>(&()).await.expect("service done");
                                        }
                                    }
                                    Ok::<_, hibana::EndpointError>(())
                                };
                            }
                            .await
                            .expect("all child results are data");
                        }
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parent_preview_can_use_a_parallel_head_before_a_child_choice()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};

    let global = g::route(
        g::send::<0, 1, Msg<0, ()>>(),
        g::par(
            g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<8, ()>>()),
            g::send::<0, 1, Msg<12, ()>>(),
        ),
    )
    .resolve::<0>();
    let (p0, p1) = (project::<0>(&global), project::<1>(&global));
    let network = in_memory::InMemoryTransport::new([]);
    let (mut slab0, mut slab1) = (vec![0; 8192], vec![0; 8192]);
    let (mut s0, mut s1) = (SessionKitStorage::uninit(), SessionKitStorage::uninit());
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    let sid = SessionId::new(93);
    let (mut controller, mut receiver) =
        (rv0.enter(sid, &p0).unwrap(), rv1.enter(sid, &p1).unwrap());

    {
        (async {
            async {
                async {
                    let _: () = (async {
                        let branch = controller.offer().await.unwrap();
                        assert_eq!(branch.label(), 12);
                        drop(branch);
                        let [mut choice, mut ordinary] = controller.par().unwrap();
                        async {
                            async {
                                choice.send::<Msg<8, ()>>(&()).await.unwrap();
                                Ok::<_, hibana::EndpointError>(())
                            };

                            async {
                                ordinary.send::<Msg<12, ()>>(&()).await.unwrap();
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let branch = receiver.offer().await.unwrap();
                        assert!([8, 12].contains(&branch.label()));
                        drop(branch);
                        let [mut choice, mut ordinary] = receiver.par().unwrap();
                        async {
                            async {
                                {
                                    let branch = choice.offer().await.unwrap();
                                    assert_eq!(branch.label(), 8);
                                    branch.recv::<Msg<8, ()>>().await.unwrap();
                                }
                                Ok::<_, hibana::EndpointError>(())
                            };

                            async {
                                ordinary.recv::<Msg<12, ()>>().await.unwrap();
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn a_parent_preview_does_not_call_a_parallel_child_resolver()
-> core::result::Result<(), hibana::EndpointError> {
    use core::cell::Cell;
    use hibana::runtime::resolver::{DecisionArm, ResolverRef};

    let child_decision = Cell::new(None);
    let global = g::route(
        g::send::<0, 1, Msg<0, ()>>(),
        g::par(
            g::route(g::send::<0, 1, Msg<4, ()>>(), g::send::<0, 1, Msg<8, ()>>()).resolve::<1>(),
            g::send::<0, 1, Msg<12, ()>>(),
        ),
    )
    .resolve::<0>();
    let (p0, p1) = (project::<0>(&global), project::<1>(&global));
    let network = in_memory::InMemoryTransport::new([]);
    let (mut slab0, mut slab1) = (vec![0; 8192], vec![0; 8192]);
    let (mut s0, mut s1) = (SessionKitStorage::uninit(), SessionKitStorage::uninit());
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Right)),
    )
    .unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<1>::decision_state(&child_decision, |decision| {
            Ok(decision.get().expect("child decision is not available yet"))
        }),
    )
    .unwrap();
    let sid = SessionId::new(94);
    let (mut controller, mut receiver) =
        (rv0.enter(sid, &p0).unwrap(), rv1.enter(sid, &p1).unwrap());

    {
        (async {
            async {
                async {
                    let _: () = (async {
                        let branch = controller.offer().await.unwrap();
                        assert_eq!(branch.label(), 12);
                        drop(branch);
                        let [mut choice, mut ordinary] = controller.par().unwrap();
                        async {
                            async {
                                {
                                    child_decision.set(Some(DecisionArm::Right));
                                    let branch = choice.offer().await.unwrap();
                                    assert_eq!(branch.label(), 8);
                                    branch.send::<Msg<8, ()>>(&()).await.unwrap();
                                }
                                Ok::<_, hibana::EndpointError>(())
                            };

                            async {
                                ordinary.send::<Msg<12, ()>>(&()).await.unwrap();
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let _: () = (async {
                        let branch = receiver.offer().await.unwrap();
                        assert!([8, 12].contains(&branch.label()));
                        drop(branch);
                        let [mut choice, mut ordinary] = receiver.par().unwrap();
                        async {
                            async {
                                {
                                    let branch = choice.offer().await.unwrap();
                                    assert_eq!(branch.label(), 8);
                                    branch.recv::<Msg<8, ()>>().await.unwrap();
                                }
                                Ok::<_, hibana::EndpointError>(())
                            };

                            async {
                                ordinary.recv::<Msg<12, ()>>().await.unwrap();
                                Ok::<_, hibana::EndpointError>(())
                            };
                        }
                        .await
                        .expect("all child results are data");
                    })
                    .await;
                    Ok::<_, hibana::EndpointError>(())
                };
            }
            .await
            .expect("all child results are data");
        })
        .await;
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_borrows_repeat_before_the_next_parallel_group()
-> core::result::Result<(), hibana::EndpointError> {
    use hibana::{Endpoint, runtime::transport::EndpointBinding};
    type Loan<'r> = Msg<4, Endpoint<'r, 3>>;
    type Return<'r> = Msg<7, Endpoint<'r, 3>>;
    type TailLoan<'r> = Msg<8, Endpoint<'r, 3>>;
    type TailReturn<'r> = Msg<11, Endpoint<'r, 3>>;

    let global = g::seq(
        g::par(
            g::seq(
                g::send::<0, 1, Msg<0, ()>>(),
                g::seq(
                    g::send::<1, 0, Msg<1, ()>>(),
                    g::seq(g::send::<0, 1, Msg<2, ()>>(), g::send::<1, 0, Msg<3, ()>>()),
                ),
            ),
            g::seq(
                g::send::<0, 1, Loan>(),
                g::seq(
                    g::send::<3, 0, Msg<5, ()>>(),
                    g::seq(g::send::<0, 3, Msg<6, ()>>(), g::send::<1, 0, Return>()),
                ),
            ),
        )
        .roll(),
        g::par(
            g::seq(
                g::send::<0, 1, TailLoan>(),
                g::seq(
                    g::send::<3, 0, Msg<9, ()>>(),
                    g::seq(
                        g::send::<0, 3, Msg<10, ()>>(),
                        g::send::<1, 0, TailReturn>(),
                    ),
                ),
            ),
            g::seq(
                g::send::<0, 1, Msg<12, ()>>(),
                g::seq(
                    g::send::<0, 2, Msg<13, ()>>(),
                    g::send::<1, 2, Msg<14, ()>>(),
                ),
            ),
        ),
    );
    let (p0, p1, p2, p3) = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
        project::<3>(&global),
    );
    let sid = SessionId::new(91);
    // SAFETY: this test owns one resource and its only initial binding.
    let network =
        in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
    let [mut slab0, mut slab1, mut slab2] =
        [vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]];
    let (mut s0, mut s1, mut s2) = (
        SessionKitStorage::uninit(),
        SessionKitStorage::uninit(),
        SessionKitStorage::uninit(),
    );
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network).unwrap();
    let mut owner = rv0.enter(sid, &p0).unwrap();
    let mut worker = rv1.enter(sid, &p1).unwrap();
    let mut collector = rv2.enter(sid, &p2).unwrap();
    let initial_db = rv0.enter(sid, &p3).unwrap();

    {
        (async {
            async {
                async {
                    let mut db = initial_db;
                    {
                        let [mut exchange, mut loan] = owner.par()?;
                        for _ in 0..2 {
                            let ((), returned) = async {
                                async {
                                    exchange.send::<Msg<0, ()>>(&()).await?;
                                    exchange.recv::<Msg<1, ()>>().await?;
                                    exchange.send::<Msg<2, ()>>(&()).await?;
                                    exchange.recv::<Msg<3, ()>>().await
                                };

                                async {
                                    loan.send::<Loan>(db).await?;
                                    loan.recv::<Msg<5, ()>>().await?;
                                    loan.send::<Msg<6, ()>>(&()).await?;
                                    loan.recv::<Return>().await
                                };
                            }
                            .await?;
                            db = returned;
                        }
                    }
                    let [mut loan, mut notice] = owner.par()?;
                    let (db, ()) = async {
                        async {
                            loan.send::<TailLoan>(db).await?;
                            loan.recv::<Msg<9, ()>>().await?;
                            loan.send::<Msg<10, ()>>(&()).await?;
                            loan.recv::<TailReturn>().await
                        };

                        async {
                            notice.send::<Msg<12, ()>>(&()).await?;
                            notice.send::<Msg<13, ()>>(&()).await
                        };
                    }
                    .await?;
                    drop(db);
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    {
                        let [mut exchange, mut loan] = worker.par()?;
                        for _ in 0..2 {
                            async {
                                async {
                                    exchange.recv::<Msg<0, ()>>().await?;
                                    exchange.send::<Msg<1, ()>>(&()).await?;
                                    exchange.recv::<Msg<2, ()>>().await?;
                                    exchange.send::<Msg<3, ()>>(&()).await
                                };

                                async {
                                    let mut db = loan.recv::<Loan>().await?;
                                    db.send::<Msg<5, ()>>(&()).await?;
                                    db.recv::<Msg<6, ()>>().await?;
                                    loan.send::<Return>(db).await
                                };
                            }
                            .await?;
                        }
                    }
                    let [mut loan, mut notice] = worker.par()?;
                    async {
                        async {
                            let mut db = loan.recv::<TailLoan>().await?;
                            db.send::<Msg<9, ()>>(&()).await?;
                            db.recv::<Msg<10, ()>>().await?;
                            loan.send::<TailReturn>(db).await
                        };

                        async {
                            notice.recv::<Msg<12, ()>>().await?;
                            notice.send::<Msg<14, ()>>(&()).await
                        };
                    }
                    .await?;
                    Ok::<_, hibana::EndpointError>(())
                };

                async {
                    let [mut notice] = collector.par()?;
                    notice.recv::<Msg<13, ()>>().await?;
                    notice.recv::<Msg<14, ()>>().await
                };
            }
            .await
        })
        .await
        .unwrap();
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn resolved_parent_waits_for_its_message_selected_child_without_blocking_a_sibling()
-> core::result::Result<(), hibana::EndpointError> {
    {
        use hibana::runtime::resolver::{DecisionArm, ResolverRef};

        let global = g::par(
            g::route(
                g::send::<0, 1, Msg<0, ()>>(),
                g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            )
            .resolve::<0>(),
            g::send::<0, 1, Msg<3, ()>>(),
        );
        let (p0, p1) = (project::<0>(&global), project::<1>(&global));
        let transport = in_memory::InMemoryTransport::new([]);
        let (mut slab0, mut slab1) = (vec![0; 32 * 1024], vec![0; 32 * 1024]);
        let (mut s0, mut s1) = (SessionKitStorage::uninit(), SessionKitStorage::uninit());
        let rv0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, transport).unwrap();
        rv0.set_resolver(
            &p0,
            ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Right)),
        )
        .unwrap();
        let sid = SessionId::new(90);
        let (mut e0, mut e1) = (rv0.enter(sid, &p0).unwrap(), rv1.enter(sid, &p1).unwrap());
        let [mut choice0, mut sibling0] = e0.par().unwrap();
        let [mut choice1, mut sibling1] = e1.par().unwrap();

        let (preview, ()) = async {
            async {
                Ok::<_, hibana::EndpointError>(
                    (crate::in_process::timeout(
                        core::time::Duration::from_millis(1),
                        choice0.offer(),
                    ))
                    .await,
                )
            };

            async {
                {
                    sibling0.send::<Msg<3, ()>>(&()).await.unwrap();
                    sibling1.recv::<Msg<3, ()>>().await.unwrap();
                }
                Ok::<_, hibana::EndpointError>(())
            };
        }
        .await
        .expect("all child results are data");
        assert!(
            preview.is_err(),
            "the child choice needs its selecting send"
        );
        drop(preview);

        (async {
            async {
                async { (choice0.send::<Msg<1, ()>>(&())).await };
                async {
                    let branch = choice1.offer().await?;
                    assert_eq!(branch.label(), 1);
                    branch.recv::<Msg<1, ()>>().await
                };
            }
            .await
        })
        .await
        .unwrap();
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn resolved_choice_reopens_after_parallel_inner_repetition()
-> core::result::Result<(), hibana::EndpointError> {
    {
        use hibana::runtime::resolver::{DecisionArm, ResolverRef};

        let repeated = g::seq(
            g::send::<1, 0, Msg<0, ()>>(),
            g::seq(
                g::send::<0, 2, Msg<1, ()>>(),
                g::seq(g::send::<2, 0, Msg<2, ()>>(), g::send::<0, 1, Msg<3, ()>>()),
            ),
        )
        .roll();
        let once = g::seq(
            g::send::<1, 0, Msg<4, ()>>(),
            g::seq(
                g::send::<0, 2, Msg<5, ()>>(),
                g::seq(g::send::<2, 0, Msg<6, ()>>(), g::send::<0, 1, Msg<7, ()>>()),
            ),
        );
        let global = g::route(
            g::par(repeated, once),
            g::seq(
                g::send::<1, 0, Msg<8, ()>>(),
                g::seq(
                    g::send::<1, 2, Msg<9, ()>>(),
                    g::send::<0, 2, Msg<10, ()>>(),
                ),
            ),
        )
        .resolve::<0>()
        .roll();
        let (p0, p1, p2) = (
            project::<0>(&global),
            project::<1>(&global),
            project::<2>(&global),
        );
        let calls = core::cell::Cell::new(0);
        let transport = in_memory::InMemoryTransport::new([]);
        let [mut slab0, mut slab1, mut slab2] =
            [vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]];
        let (mut s0, mut s1, mut s2) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let rv0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
        let rv1 = s1.init().rendezvous(&mut slab1, transport.clone()).unwrap();
        let rv2 = s2.init().rendezvous(&mut slab2, transport.clone()).unwrap();
        rv1.set_resolver(
            &p1,
            ResolverRef::<0>::decision_state(&calls, |calls| {
                calls.set(calls.get() + 1);
                Ok(DecisionArm::Left)
            }),
        )
        .unwrap();
        let sid = SessionId::new(89);
        let (mut e0, mut e1, mut e2) = (
            rv0.enter(sid, &p0).unwrap(),
            rv1.enter(sid, &p1).unwrap(),
            rv2.enter(sid, &p2).unwrap(),
        );
        let run0 = async {
            for _ in 0..2 {
                let branch = e0.offer().await?;
                assert!([0, 4].contains(&branch.label()));
                drop(branch);
                let [mut repeated, mut once] = e0.par()?;
                async {
                    async {
                        for _ in 0..2 {
                            repeated.recv::<Msg<0, ()>>().await?;
                            repeated.send::<Msg<1, ()>>(&()).await?;
                            repeated.recv::<Msg<2, ()>>().await?;
                            repeated.send::<Msg<3, ()>>(&()).await?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        once.recv::<Msg<4, ()>>().await?;
                        once.send::<Msg<5, ()>>(&()).await?;
                        once.recv::<Msg<6, ()>>().await?;
                        once.send::<Msg<7, ()>>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await?;
            }
            Ok::<_, hibana::EndpointError>(())
        };
        let run1 = async {
            for visit in 0..2 {
                let branch = e1.offer().await?;
                let label = branch.label();
                assert!([0, 4].contains(&label));
                drop(branch);
                let branch = e1.offer().await?;
                assert_eq!(branch.label(), label);
                drop(branch);
                assert_eq!(calls.get(), visit + 1);
                let [mut repeated, mut once] = e1.par()?;
                async {
                    async {
                        for _ in 0..2 {
                            hibana_runtime::yield_now().await;
                            repeated.send::<Msg<0, ()>>(&()).await?;
                            repeated.recv::<Msg<3, ()>>().await?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        once.send::<Msg<4, ()>>(&()).await?;
                        once.recv::<Msg<7, ()>>().await?;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await?;
            }
            Ok::<_, hibana::EndpointError>(())
        };
        let run2 = async {
            for _ in 0..2 {
                let branch = e2.offer().await?;
                assert!([1, 5].contains(&branch.label()));
                drop(branch);
                let [mut repeated, mut once] = e2.par()?;
                async {
                    async {
                        for _ in 0..2 {
                            repeated.recv::<Msg<1, ()>>().await?;
                            repeated.send::<Msg<2, ()>>(&()).await?;
                        }
                        Ok::<_, hibana::EndpointError>(())
                    };

                    async {
                        once.recv::<Msg<5, ()>>().await?;
                        once.send::<Msg<6, ()>>(&()).await?;
                        Ok::<_, hibana::EndpointError>(())
                    };
                }
                .await?;
            }
            Ok::<_, hibana::EndpointError>(())
        };
        (async {
            async {
                async { (run0).await };
                async { (run1).await };
                async { (run2).await };
            }
            .await
        })
        .await
        .unwrap();
        assert_eq!(calls.get(), 2);
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_choice_remains_available_after_its_outer_parallel_sibling_finishes()
-> core::result::Result<(), hibana::EndpointError> {
    {
        use hibana::{
            Endpoint,
            runtime::{
                resolver::{DecisionArm, ResolverRef},
                transport::EndpointBinding,
            },
        };

        type Loan<'r> = Msg<0, Endpoint<'r, 3>>;
        type Return<'r> = Msg<3, Endpoint<'r, 3>>;
        let loan = g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(
                g::send::<3, 2, Msg<1, ()>>(),
                g::seq(g::send::<2, 3, Msg<2, ()>>(), g::send::<1, 0, Return>()),
            ),
        );
        let choice = g::route(
            g::seq(
                g::send::<0, 1, Msg<4, ()>>(),
                g::seq(g::send::<0, 2, Msg<5, ()>>(), g::send::<1, 2, Msg<6, ()>>()),
            ),
            g::seq(
                g::send::<0, 1, Msg<7, ()>>(),
                g::seq(
                    g::send::<1, 2, Msg<8, ()>>(),
                    g::seq(
                        g::send::<2, 1, Msg<9, ()>>(),
                        g::send::<1, 0, Msg<10, ()>>(),
                    ),
                ),
            ),
        );
        let sibling = g::seq(
            g::send::<0, 1, Msg<11, ()>>(),
            g::seq(
                g::send::<1, 2, Msg<12, ()>>(),
                g::seq(
                    g::send::<2, 1, Msg<13, ()>>(),
                    g::send::<1, 0, Msg<14, ()>>(),
                ),
            ),
        );
        let global = g::route(
            g::par(g::par(loan, choice), sibling),
            g::seq(
                g::send::<0, 1, Msg<15, Endpoint<'_, 3>>>(),
                g::seq(
                    g::send::<3, 2, Msg<16, ()>>(),
                    g::seq(
                        g::send::<2, 3, Msg<17, ()>>(),
                        g::send::<1, 0, Msg<18, Endpoint<'_, 3>>>(),
                    ),
                ),
            ),
        )
        .resolve::<0>();
        let (p0, p1, p2, p3) = (
            project::<0>(&global),
            project::<1>(&global),
            project::<2>(&global),
            project::<3>(&global),
        );
        let sid = SessionId::new(87);
        // SAFETY: this test owns one resource and its sole initial binding.
        let transport =
            in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
        let (mut slab0, mut slab1, mut slab2) =
            (vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]);
        let (mut s0, mut s1, mut s2) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let r0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
        let r1 = s1.init().rendezvous(&mut slab1, transport.clone()).unwrap();
        let r2 = s2.init().rendezvous(&mut slab2, transport).unwrap();
        r0.set_resolver(
            &p0,
            ResolverRef::<0>::decision_state(&(), |_| Ok(DecisionArm::Left)),
        )
        .unwrap();
        let mut owner = r0.enter(sid, &p0).unwrap();
        let mut worker = r1.enter(sid, &p1).unwrap();
        let mut service = r2.enter(sid, &p2).unwrap();
        let db = r0.enter(sid, &p3).unwrap();

        owner.send::<Msg<11, ()>>(&()).await.unwrap();
        worker
            .offer()
            .await
            .unwrap()
            .recv::<Msg<11, ()>>()
            .await
            .unwrap();
        worker.send::<Msg<12, ()>>(&()).await.unwrap();
        service.recv::<Msg<12, ()>>().await.unwrap();
        service.send::<Msg<13, ()>>(&()).await.unwrap();
        worker.recv::<Msg<13, ()>>().await.unwrap();
        worker.send::<Msg<14, ()>>(&()).await.unwrap();
        owner.recv::<Msg<14, ()>>().await.unwrap();

        owner.send::<Msg<4, ()>>(&()).await.unwrap();
        let branch = worker.offer().await.unwrap();
        assert_eq!(branch.label(), 4);
        drop(branch);
        worker
            .offer()
            .await
            .unwrap()
            .recv::<Msg<4, ()>>()
            .await
            .unwrap();
        owner.send::<Msg<5, ()>>(&()).await.unwrap();
        service.recv::<Msg<5, ()>>().await.unwrap();
        worker.send::<Msg<6, ()>>(&()).await.unwrap();
        service.recv::<Msg<6, ()>>().await.unwrap();

        owner.send::<Loan>(db).await.unwrap();
        let mut db = worker.recv::<Loan>().await.unwrap();
        db.send::<Msg<1, ()>>(&()).await.unwrap();
        service.recv::<Msg<1, ()>>().await.unwrap();
        service.send::<Msg<2, ()>>(&()).await.unwrap();
        db.recv::<Msg<2, ()>>().await.unwrap();
        worker.send::<Return>(db).await.unwrap();
        let _db = owner.recv::<Return>().await.unwrap();
    };

    Ok(())
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn parallel_choice_is_observable_before_a_sequential_resolver()
-> core::result::Result<(), hibana::EndpointError> {
    {
        use hibana::{
            Endpoint,
            runtime::{
                resolver::{DecisionArm, ResolverRef},
                transport::EndpointBinding,
            },
        };

        let choice = g::route(
            g::seq(
                g::send::<0, 1, Msg<0, ()>>(),
                g::seq(g::send::<0, 2, Msg<1, ()>>(), g::send::<1, 2, Msg<2, ()>>()),
            ),
            g::seq(
                g::send::<0, 1, Msg<3, ()>>(),
                g::seq(g::send::<0, 2, Msg<4, ()>>(), g::send::<1, 2, Msg<5, ()>>()),
            ),
        );
        let loan = g::seq(
            g::send::<0, 1, Msg<6, Endpoint<'_, 3>>>(),
            g::seq(
                g::send::<3, 2, Msg<7, ()>>(),
                g::seq(
                    g::send::<2, 3, Msg<8, ()>>(),
                    g::send::<1, 0, Msg<9, Endpoint<'_, 3>>>(),
                ),
            ),
        );
        let next = g::route(
            g::seq(
                g::send::<0, 1, Msg<10, Endpoint<'_, 3>>>(),
                g::seq(
                    g::send::<3, 2, Msg<11, ()>>(),
                    g::seq(
                        g::send::<2, 3, Msg<12, ()>>(),
                        g::send::<1, 0, Msg<13, Endpoint<'_, 3>>>(),
                    ),
                ),
            ),
            g::seq(
                g::send::<0, 1, Msg<14, Endpoint<'_, 3>>>(),
                g::seq(
                    g::send::<3, 2, Msg<15, ()>>(),
                    g::seq(
                        g::send::<2, 3, Msg<16, ()>>(),
                        g::send::<1, 0, Msg<17, Endpoint<'_, 3>>>(),
                    ),
                ),
            ),
        )
        .resolve::<0>();
        let global = g::seq(g::par(choice, loan), next);
        let (p0, p1, p2, p3) = (
            project::<0>(&global),
            project::<1>(&global),
            project::<2>(&global),
            project::<3>(&global),
        );
        let sid = SessionId::new(88);
        let calls = core::cell::Cell::new(0);
        // SAFETY: this test owns one resource and its sole initial binding.
        let transport =
            in_memory::InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
        let (mut slab0, mut slab1, mut slab2) =
            (vec![0; 64 * 1024], vec![0; 64 * 1024], vec![0; 64 * 1024]);
        let (mut s0, mut s1, mut s2) = (
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
            SessionKitStorage::uninit(),
        );
        let r0 = s0.init().rendezvous(&mut slab0, transport.clone()).unwrap();
        let r1 = s1.init().rendezvous(&mut slab1, transport.clone()).unwrap();
        let r2 = s2.init().rendezvous(&mut slab2, transport).unwrap();
        r0.set_resolver(
            &p0,
            ResolverRef::<0>::decision_state(&calls, |calls| {
                calls.set(calls.get() + 1);
                Ok(DecisionArm::Left)
            }),
        )
        .unwrap();
        let mut owner = r0.enter(sid, &p0).unwrap();
        let mut worker = r1.enter(sid, &p1).unwrap();
        let mut service = r2.enter(sid, &p2).unwrap();
        let db = r0.enter(sid, &p3).unwrap();

        owner.send::<Msg<6, Endpoint<'_, 3>>>(db).await.unwrap();
        let mut db = worker.recv::<Msg<6, Endpoint<'_, 3>>>().await.unwrap();
        db.send::<Msg<7, ()>>(&()).await.unwrap();
        service.recv::<Msg<7, ()>>().await.unwrap();
        service.send::<Msg<8, ()>>(&()).await.unwrap();
        db.recv::<Msg<8, ()>>().await.unwrap();
        worker.send::<Msg<9, Endpoint<'_, 3>>>(db).await.unwrap();
        let db = owner.recv::<Msg<9, Endpoint<'_, 3>>>().await.unwrap();
        owner.send::<Msg<0, ()>>(&()).await.unwrap();
        assert_eq!(calls.get(), 0, "the join has not completed");
        let branch = (worker.offer()).await.unwrap();
        assert_eq!(branch.label(), 0);
        branch.recv::<Msg<0, ()>>().await.unwrap();
        owner.send::<Msg<1, ()>>(&()).await.unwrap();
        service.recv::<Msg<1, ()>>().await.unwrap();
        worker.send::<Msg<2, ()>>(&()).await.unwrap();
        service.recv::<Msg<2, ()>>().await.unwrap();
        assert_eq!(calls.get(), 0);

        owner.send::<Msg<10, Endpoint<'_, 3>>>(db).await.unwrap();
        assert_eq!(calls.get(), 1);
        let mut db = worker
            .offer()
            .await
            .unwrap()
            .recv::<Msg<10, Endpoint<'_, 3>>>()
            .await
            .unwrap();
        db.send::<Msg<11, ()>>(&()).await.unwrap();
        service.recv::<Msg<11, ()>>().await.unwrap();
        service.send::<Msg<12, ()>>(&()).await.unwrap();
        db.recv::<Msg<12, ()>>().await.unwrap();
        worker.send::<Msg<13, Endpoint<'_, 3>>>(db).await.unwrap();
        let _db = owner.recv::<Msg<13, Endpoint<'_, 3>>>().await.unwrap();
    };

    Ok(())
}

const LOCAL_ROLE: u8 = 1;
const WORKER_ROLE: u8 = 2;
const SIDE_ROLE: u8 = 3;
const OBSERVER_ROLE: u8 = 4;

const ROUTE_PAYLOAD: u8 = 173;
const ROUTE_OTHER: u8 = 174;
const SIDE_REQ: u8 = 175;
const SIDE_RET: u8 = 176;
const JOIN: u8 = 177;
const PAR_A: u8 = 201;
const PAR_B: u8 = 202;
const PAR_D: u8 = 203;
const PAR_E: u8 = 204;
const PAR_POST: u8 = 205;
const ROUTE_PAR_A: u8 = 221;
const ROUTE_PAR_B: u8 = 222;
const ROUTE_PAR_C: u8 = 223;
const ROUTE_PAR_D: u8 = 224;
const ROUTE_PAR_R: u8 = 225;
const ROUTE_PAR_POST: u8 = 226;
const ROUTE_PAR_RIGHT_SIDE: u8 = 233;
const ROUTE_PAR_RIGHT_OBSERVER: u8 = 234;
const DEAD_RIGHT_A: u8 = 227;
const DEAD_RIGHT_B: u8 = 228;
const DEAD_RIGHT_C: u8 = 229;
const DEAD_RIGHT_E: u8 = 230;
const DEAD_RIGHT_D: u8 = 231;
const DEAD_RIGHT_POST: u8 = 232;

fn program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let routed = g::route(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAYLOAD, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_OTHER, u8>>(),
    );
    let side = g::seq(
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<SIDE_REQ, u8>>(),
        g::send::<SIDE_ROLE, LOCAL_ROLE, Msg<SIDE_RET, u8>>(),
    );
    project(&g::seq(
        g::par(routed, side),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<JOIN, u8>>(),
    ))
}

fn nested_parallel_join_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let inner = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<PAR_A, u8>>(),
        g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<PAR_B, u8>>(),
    );
    let left = g::seq(inner, g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<PAR_D, u8>>());
    let right = g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<PAR_E, u8>>();
    project(&g::seq(
        g::par(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<PAR_POST, u8>>(),
    ))
}

fn route_left_nested_parallel_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let nested_join = g::par(
        g::par(
            g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_A, u8>>(),
            g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<ROUTE_PAR_B, u8>>(),
        ),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_C, u8>>(),
    );
    let left = g::seq(
        nested_join,
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_D, u8>>(),
    );
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<ROUTE_PAR_R, u8>>(),
        g::par(
            g::send::<LOCAL_ROLE, SIDE_ROLE, Msg<ROUTE_PAR_RIGHT_SIDE, u8>>(),
            g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_RIGHT_OBSERVER, u8>>(),
        ),
    );
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<ROUTE_PAR_POST, u8>>(),
    ))
}

fn route_right_parallel_dead_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn parallel_route_right_parallel_dead_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let right = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    let routed = g::route(left, right);
    let sibling = g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_E, u8>>();
    project(&g::seq(
        g::par(routed, sibling),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn outer_left_kills_nested_right_route_program<const ROLE: u8>() -> RoleProgram<ROLE> {
    let left = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_A, u8>>();
    let inner_left = g::par(
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_B, u8>>(),
        g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_C, u8>>(),
    );
    let inner_right = g::send::<LOCAL_ROLE, WORKER_ROLE, Msg<DEAD_RIGHT_D, u8>>();
    let right = g::route(inner_left, inner_right);
    project(&g::seq(
        g::route(left, right),
        g::send::<LOCAL_ROLE, OBSERVER_ROLE, Msg<DEAD_RIGHT_POST, u8>>(),
    ))
}

fn assert_join_blocked(rendered: &str) {
    assert!(
        rendered.contains("LabelMismatch") || rendered.contains("OutOfOrder"),
        "post-par join must be rejected by resident progress evidence: {rendered}"
    );
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn unselected_route_arm_parallel_events_are_dead_and_not_join_obligations() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            {
                let (mut local, mut worker, observer) = {
                    let sid = SessionId::new(98);
                    (
                        rv.enter(sid, &route_right_parallel_dead_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                {
                    let err = local
                        .send::<Msg<DEAD_RIGHT_B, u8>>(&0)
                        .await
                        .expect_err("unselected right nested-par B must be dead");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, observer) = {
                    let sid = SessionId::new(99);
                    (
                        rv.enter(sid, &route_right_parallel_dead_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                {
                    let err = local
                        .send::<Msg<DEAD_RIGHT_C, u8>>(&0)
                        .await
                        .expect_err("unselected right nested-par C must be dead");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, mut observer) = {
                    let sid = SessionId::new(100);
                    (
                        rv.enter(sid, &route_right_parallel_dead_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_right_parallel_dead_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                local
                    .send::<Msg<DEAD_RIGHT_POST, u8>>(&2)
                    .await
                    .expect("send post route");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post"),
                    2
                );
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn unselected_route_arm_parallel_events_do_not_block_parallel_join() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            {
                let (mut local, mut worker, observer) = {
                    let sid = SessionId::new(101);
                    (
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<LOCAL_ROLE>(),
                        )
                        .expect("attach LOCAL_ROLE"),
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<WORKER_ROLE>(),
                        )
                        .expect("attach WORKER_ROLE"),
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<OBSERVER_ROLE>(),
                        )
                        .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                {
                    let err = local
                        .send::<Msg<DEAD_RIGHT_POST, u8>>(&0)
                        .await
                        .expect_err("outer par join must still wait for sibling E");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, mut observer) = {
                    let sid = SessionId::new(102);
                    (
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<LOCAL_ROLE>(),
                        )
                        .expect("attach LOCAL_ROLE"),
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<WORKER_ROLE>(),
                        )
                        .expect("attach WORKER_ROLE"),
                        rv.enter(
                            sid,
                            &parallel_route_right_parallel_dead_program::<OBSERVER_ROLE>(),
                        )
                        .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                local
                    .send::<Msg<DEAD_RIGHT_E, u8>>(&2)
                    .await
                    .expect("send E");
                local
                    .send::<Msg<DEAD_RIGHT_POST, u8>>(&3)
                    .await
                    .expect("send post");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_E, u8>>()
                        .await
                        .expect("recv E"),
                    2
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post"),
                    3
                );
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn outer_left_selection_kills_nested_right_route_and_parallel_body() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            for (sid, label) in [
                (103, DEAD_RIGHT_B),
                (104, DEAD_RIGHT_C),
                (105, DEAD_RIGHT_D),
            ] {
                let (mut local, mut worker, observer) = {
                    let sid = SessionId::new(sid);
                    (
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<LOCAL_ROLE>(),
                        )
                        .expect("attach LOCAL_ROLE"),
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<WORKER_ROLE>(),
                        )
                        .expect("attach WORKER_ROLE"),
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<OBSERVER_ROLE>(),
                        )
                        .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                let rejected = match label {
                    DEAD_RIGHT_B => local.send::<Msg<DEAD_RIGHT_B, u8>>(&0).await,
                    DEAD_RIGHT_C => local.send::<Msg<DEAD_RIGHT_C, u8>>(&0).await,
                    DEAD_RIGHT_D => local.send::<Msg<DEAD_RIGHT_D, u8>>(&0).await,
                    _ => unreachable!(),
                }
                .expect_err("nested right event must be dead after outer left selection");
                assert_join_blocked(&format!("{rejected:?}"));
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, mut observer) = {
                    let sid = SessionId::new(106);
                    (
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<LOCAL_ROLE>(),
                        )
                        .expect("attach LOCAL_ROLE"),
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<WORKER_ROLE>(),
                        )
                        .expect("attach WORKER_ROLE"),
                        rv.enter(
                            sid,
                            &outer_left_kills_nested_right_route_program::<OBSERVER_ROLE>(),
                        )
                        .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<DEAD_RIGHT_A, u8>>(&1)
                    .await
                    .expect("send A");
                local
                    .send::<Msg<DEAD_RIGHT_POST, u8>>(&2)
                    .await
                    .expect("send post route");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch
                        .recv::<Msg<DEAD_RIGHT_A, u8>>()
                        .await
                        .expect("recv A"),
                    1
                );
                assert_eq!(
                    observer
                        .recv::<Msg<DEAD_RIGHT_POST, u8>>()
                        .await
                        .expect("recv post"),
                    2
                );
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn route_selected_left_keeps_entire_nested_parallel_path_live() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            {
                let (mut local, mut worker, side, observer) = {
                    let sid = SessionId::new(107);
                    (
                        rv.enter(sid, &route_left_nested_parallel_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                    1
                );
                {
                    let err = local
                        .send::<Msg<ROUTE_PAR_R, u8>>(&0)
                        .await
                        .expect_err("right arm must be unselected after A commits");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box((side, observer));
            }
            {
                let (mut local, mut worker, side, observer) = {
                    let sid = SessionId::new(108);
                    (
                        rv.enter(sid, &route_left_nested_parallel_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                    .await
                    .expect("send A");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                    1
                );
                {
                    let err = local
                        .send::<Msg<ROUTE_PAR_D, u8>>(&0)
                        .await
                        .expect_err("D must wait for B and C");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box((side, observer));
            }
            {
                let (mut local, mut worker, mut side, observer) = {
                    let sid = SessionId::new(109);
                    (
                        rv.enter(sid, &route_left_nested_parallel_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                    .await
                    .expect("send A");
                local
                    .send::<Msg<ROUTE_PAR_B, u8>>(&2)
                    .await
                    .expect("send B");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                    1
                );
                assert_eq!(
                    side.recv::<Msg<ROUTE_PAR_B, u8>>().await.expect("recv B"),
                    2
                );
                {
                    let err = local
                        .send::<Msg<ROUTE_PAR_D, u8>>(&0)
                        .await
                        .expect_err("D must still wait for C");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, mut side, mut observer) = {
                    let sid = SessionId::new(110);
                    (
                        rv.enter(sid, &route_left_nested_parallel_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &route_left_nested_parallel_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAR_A, u8>>(&1)
                    .await
                    .expect("send A");
                local
                    .send::<Msg<ROUTE_PAR_B, u8>>(&2)
                    .await
                    .expect("send B");
                local
                    .send::<Msg<ROUTE_PAR_C, u8>>(&3)
                    .await
                    .expect("send C");
                local
                    .send::<Msg<ROUTE_PAR_D, u8>>(&4)
                    .await
                    .expect("send D");
                local
                    .send::<Msg<ROUTE_PAR_POST, u8>>(&5)
                    .await
                    .expect("send Post");
                let branch = worker.offer().await.expect("offer A");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAR_A, u8>>().await.expect("recv A"),
                    1
                );
                assert_eq!(
                    side.recv::<Msg<ROUTE_PAR_B, u8>>().await.expect("recv B"),
                    2
                );
                assert_eq!(
                    observer
                        .recv::<Msg<ROUTE_PAR_C, u8>>()
                        .await
                        .expect("recv C"),
                    3
                );
                assert_eq!(
                    worker.recv::<Msg<ROUTE_PAR_D, u8>>().await.expect("recv D"),
                    4
                );
                assert_eq!(
                    observer
                        .recv::<Msg<ROUTE_PAR_POST, u8>>()
                        .await
                        .expect("recv Post"),
                    5
                );
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn route_inside_parallel_lane_cannot_release_join_before_sibling_lane() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            {
                let (mut local, mut worker, side, observer) = {
                    let sid = SessionId::new(111);
                    (
                        rv.enter(sid, &program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                    .await
                    .expect("send route payload");
                let branch = worker.offer().await.expect("offer route payload");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                    10
                );
                {
                    let err = local
                        .send::<Msg<JOIN, u8>>(&0)
                        .await
                        .expect_err("join must wait for the sibling parallel lane");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box((side, observer));
            }
            {
                let (mut local, mut worker, mut side, observer) = {
                    let sid = SessionId::new(112);
                    (
                        rv.enter(sid, &program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                    .await
                    .expect("send route payload");
                local
                    .send::<Msg<SIDE_REQ, u8>>(&20)
                    .await
                    .expect("send side");
                let branch = worker.offer().await.expect("offer route payload");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                    10
                );
                assert_eq!(
                    side.recv::<Msg<SIDE_REQ, u8>>().await.expect("recv side"),
                    20
                );
                {
                    let err = local
                        .send::<Msg<JOIN, u8>>(&0)
                        .await
                        .expect_err("join must wait for the sibling lane response");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box(observer);
            }
            {
                let (mut local, mut worker, mut side, mut observer) = {
                    let sid = SessionId::new(113);
                    (
                        rv.enter(sid, &program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local
                    .send::<Msg<ROUTE_PAYLOAD, u8>>(&10)
                    .await
                    .expect("send route payload");
                local
                    .send::<Msg<SIDE_REQ, u8>>(&20)
                    .await
                    .expect("send side");
                let branch = worker.offer().await.expect("offer route payload");
                assert_eq!(
                    branch.recv::<Msg<ROUTE_PAYLOAD, u8>>().await.expect("recv"),
                    10
                );
                assert_eq!(
                    side.recv::<Msg<SIDE_REQ, u8>>().await.expect("recv side"),
                    20
                );
                side.send::<Msg<SIDE_RET, u8>>(&30)
                    .await
                    .expect("send response");
                assert_eq!(
                    local
                        .recv::<Msg<SIDE_RET, u8>>()
                        .await
                        .expect("recv response"),
                    30
                );
                local.send::<Msg<JOIN, u8>>(&40).await.expect("send join");
                assert_eq!(
                    observer.recv::<Msg<JOIN, u8>>().await.expect("recv join"),
                    40
                );
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_parallel_join_requires_every_dependency_before_post() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        (async {
            {
                let (mut local, worker, side, mut observer) = {
                    let sid = SessionId::new(114);
                    (
                        rv.enter(sid, &nested_parallel_join_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local.send::<Msg<PAR_E, u8>>(&4).await.expect("send E");
                assert_eq!(observer.recv::<Msg<PAR_E, u8>>().await.expect("recv E"), 4);
                {
                    let err = local
                        .send::<Msg<PAR_POST, u8>>(&0)
                        .await
                        .expect_err("Post must still wait for the left parallel branch");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box((worker, side));
            }
            {
                let (mut local, mut worker, side, observer) = {
                    let sid = SessionId::new(115);
                    (
                        rv.enter(sid, &nested_parallel_join_program::<LOCAL_ROLE>())
                            .expect("attach LOCAL_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<WORKER_ROLE>())
                            .expect("attach WORKER_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<SIDE_ROLE>())
                            .expect("attach SIDE_ROLE"),
                        rv.enter(sid, &nested_parallel_join_program::<OBSERVER_ROLE>())
                            .expect("attach OBSERVER_ROLE"),
                    )
                };
                local.send::<Msg<PAR_A, u8>>(&1).await.expect("send A");
                assert_eq!(worker.recv::<Msg<PAR_A, u8>>().await.expect("recv A"), 1);
                {
                    let err = local
                        .send::<Msg<PAR_D, u8>>(&0)
                        .await
                        .expect_err("D must wait for B");
                    assert_join_blocked(&format!("{err:?}"));
                }
                core::hint::black_box((side, observer));
            }
        })
        .await;
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn nested_parallel_join_commits_post_after_sibling_first_completion() {
    let mut slab = [0; 64 * 1024];
    {
        let mut storage = SessionKitStorage::uninit();
        let cluster = storage.init();
        let transport = TestTransport::new();
        let rv = cluster
            .rendezvous(&mut slab, transport)
            .expect("register rendezvous");
        let sid = SessionId::new(94);

        let mut local = rv
            .enter(sid, &nested_parallel_join_program::<LOCAL_ROLE>())
            .expect("attach local role");
        let mut worker = rv
            .enter(sid, &nested_parallel_join_program::<WORKER_ROLE>())
            .expect("attach worker role");
        let mut side = rv
            .enter(sid, &nested_parallel_join_program::<SIDE_ROLE>())
            .expect("attach side role");
        let mut observer = rv
            .enter(sid, &nested_parallel_join_program::<OBSERVER_ROLE>())
            .expect("attach observer role");

        {
            local
                .send::<Msg<PAR_E, u8>>(&4)
                .await
                .expect("send E before nested left branch completes");
            local.send::<Msg<PAR_A, u8>>(&1).await.expect("send A");
            local.send::<Msg<PAR_B, u8>>(&2).await.expect("send B");
            local.send::<Msg<PAR_D, u8>>(&3).await.expect("send D");
            local
                .send::<Msg<PAR_POST, u8>>(&5)
                .await
                .expect("send Post");

            assert_eq!(worker.recv::<Msg<PAR_A, u8>>().await.expect("recv A"), 1);
            assert_eq!(side.recv::<Msg<PAR_B, u8>>().await.expect("recv B"), 2);
            assert_eq!(worker.recv::<Msg<PAR_D, u8>>().await.expect("recv D"), 3);
            assert_eq!(observer.recv::<Msg<PAR_E, u8>>().await.expect("recv E"), 4);
            assert_eq!(
                observer
                    .recv::<Msg<PAR_POST, u8>>()
                    .await
                    .expect("recv Post"),
                5
            );
        }
    };
}

#[hibana_runtime::main(crate::in_process::InProcess::default())]
#[test]
async fn independent_rolled_admission_send_is_enabled_while_abi_input_is_quiet() {
    let bootstrap = g::seq(
        g::seq(g::send::<2, 0, Msg<1, ()>>(), g::send::<0, 2, Msg<2, ()>>()),
        g::seq(g::send::<2, 1, Msg<3, ()>>(), g::send::<1, 2, Msg<4, ()>>()),
    );
    let abi = g::route(
        g::seq(g::send::<1, 0, Msg<5, ()>>(), g::send::<0, 1, Msg<6, ()>>()),
        g::seq(g::send::<1, 0, Msg<7, ()>>(), g::send::<0, 1, Msg<8, ()>>()),
    )
    .roll();
    let admission = g::seq(
        g::send::<0, 2, Msg<9, ()>>(),
        g::route(
            g::send::<2, 0, Msg<10, ()>>(),
            g::send::<2, 0, Msg<11, ()>>(),
        ),
    )
    .roll();
    let body = g::seq(
        bootstrap,
        g::par(
            g::par(
                abi,
                g::par(
                    g::send::<0, 2, Msg<12, ()>>(),
                    g::send::<0, 2, Msg<13, ()>>(),
                ),
            ),
            admission,
        ),
    );
    let app_program: RoleProgram<0> = project(&body);
    let abi_program: RoleProgram<1> = project(&body);
    let owner_program: RoleProgram<2> = project(&body);
    let mut slab = [0u8; 16384];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let transport = TestTransport::new();
    let rendezvous = kit.rendezvous(&mut slab, transport.clone()).unwrap();
    let sid = SessionId::new(123);
    let mut app = rendezvous.enter(sid, &app_program).unwrap();
    let mut abi = rendezvous.enter(sid, &abi_program).unwrap();
    let mut owner = rendezvous.enter(sid, &owner_program).unwrap();
    {
        owner.send::<Msg<1, ()>>(&()).await.unwrap();
        app.recv::<Msg<1, ()>>().await.unwrap();
        app.send::<Msg<2, ()>>(&()).await.unwrap();
        owner.recv::<Msg<2, ()>>().await.unwrap();
        owner.send::<Msg<3, ()>>(&()).await.unwrap();
        abi.recv::<Msg<3, ()>>().await.unwrap();
        abi.send::<Msg<4, ()>>(&()).await.unwrap();
        owner.recv::<Msg<4, ()>>().await.unwrap();
        app.send::<Msg<9, ()>>(&()).await.unwrap();
        owner.recv::<Msg<9, ()>>().await.unwrap();
        owner.send::<Msg<10, ()>>(&()).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<10, ()>>()
            .await
            .unwrap();
        // Return to the untouched ABI lane after another rolled region moved
        // the shared cursor. Its alternate request must still select a branch.
        abi.send::<Msg<7, ()>>(&()).await.unwrap();
        app.offer()
            .await
            .unwrap()
            .recv::<Msg<7, ()>>()
            .await
            .unwrap();
        app.send::<Msg<8, ()>>(&()).await.unwrap();
        abi.recv::<Msg<8, ()>>().await.unwrap();
    }
    assert!(transport.queue_is_empty());
}
