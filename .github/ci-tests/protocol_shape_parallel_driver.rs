// Test operands are real whole endpoints or their public parallel borrows.
enum Fragment<'p, 'r, const ROLE: u8> {
    Whole(&'p mut Endpoint<'r, ROLE>),
    Parallel(hibana::ParallelEndpoint<'p, 'r, ROLE>),
}

#[derive(Default)]
struct PendingCalls {
    calls: std::cell::RefCell<std::collections::BTreeSet<(u8, usize)>>,
    overlapped: std::cell::Cell<bool>,
}

async fn record_pending<F: core::future::Future>(
    future: F,
    observed: &PendingCalls,
    caller: (u8, usize),
) -> F::Output {
    tokio::pin!(future);
    core::future::poll_fn(|cx| match future.as_mut().poll(cx) {
        core::task::Poll::Pending => {
            let mut calls = observed.calls.borrow_mut();
            calls.insert(caller);
            if calls
                .iter()
                .any(|other| other.0 == caller.0 && *other != caller)
            {
                observed.overlapped.set(true);
            }
            core::task::Poll::Pending
        }
        core::task::Poll::Ready(output) => {
            observed.calls.borrow_mut().remove(&caller);
            core::task::Poll::Ready(output)
        }
    })
    .await
}

fn fragments<'p, 'r, const ROLE: u8>(
    endpoint: &'p mut Endpoint<'r, ROLE>,
    count: usize,
) -> Vec<Fragment<'p, 'r, ROLE>> {
    match count {
        0 => Vec::new(),
        1 => vec![Fragment::Whole(endpoint)],
        2 => split_widths!(),
        _ => panic!("parallel width outside source inventory"),
    }
}

// In this generated grammar the worker performs the resource operations
// between receiving and returning the loan. The loan itself moves as a value.
fn participant(command: &Command, owner: u8) -> u8 {
    let role = if command.receive {
        command.message.to
    } else {
        command.message.from
    };
    if role == 3 { owner ^ 1 } else { role }
}

async fn drive_participant<'p, 'r, const ROLE: u8>(
    mut endpoint: Option<Fragment<'p, 'r, ROLE>>,
    mut resource: Option<Endpoint<'r, 3>>,
    commands: &[Command],
    decision_input: (&Decisions, &[u16], u8),
    observed: &PendingCalls,
    fragment: usize,
) -> Option<Endpoint<'r, 3>> {
    // Let receivers actually wait on distinct borrows before the first sends.
    if ROLE == decision_input.2 {
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
    }
    for command in commands
        .iter()
        .filter(|c| participant(c, decision_input.2) == ROLE)
    {
        tokio::task::yield_now().await;
        if ROLE == decision_input.2 {
            decision_input.0.prepare(decision_input.1, command.message);
        }
        macro_rules! send {
            ($message:ty, $payload:expr) => {
                match endpoint.as_mut().expect("missing participant fragment") {
                    Fragment::Whole(endpoint) => endpoint.send::<$message>($payload).await,
                    Fragment::Parallel(endpoint) => endpoint.send::<$message>($payload).await,
                }
            };
        }
        macro_rules! recv {
            ($message:ty) => {
                match endpoint.as_mut().expect("missing participant fragment") {
                    Fragment::Whole(endpoint) => receive!(&mut **endpoint, $message, command).await,
                    Fragment::Parallel(endpoint) => receive!(endpoint, $message, command).await,
                }
            };
        }
        macro_rules! execute {
            ($label:literal) => {{
                type Unit = g::Msg<$label, ()>;
                type Loan<'a> = g::Msg<$label, Endpoint<'a, 3>>;
                let role = if command.receive {
                    command.message.to
                } else {
                    command.message.from
                };
                match (role, command.receive, command.message.owned) {
                    (_, false, true) => {
                        send!(Loan<'_>, resource.take().expect("missing loan operand")).unwrap()
                    }
                    (_, true, true) => {
                        let loan = recv!(Loan<'_>).unwrap();
                        assert!(resource.replace(loan).is_none());
                    }
                    (3, false, false) => {
                        resource.as_mut().unwrap().send::<Unit>(&()).await.unwrap()
                    }
                    (3, true, false) => receive!(resource.as_mut().unwrap(), Unit, command)
                        .await
                        .unwrap(),
                    (_, false, false) => send!(Unit, &()).unwrap(),
                    (_, true, false) => recv!(Unit).unwrap(),
                }
            }};
        }
        record_pending(
            async {
                match command.message.label {
                    0 => dispatch_labels!(),
                    _ => panic!("parallel label outside source inventory"),
                }
            },
            observed,
            (ROLE, fragment),
        )
        .await;
    }
    resource
}

async fn parallel_replay(
    programs: &Programs,
    resolver_count: u16,
    decisions: &Decisions,
    messages: &[Message],
    commands: &[Command],
    groups: &[&[u8]],
) {
    assert!(groups.len() >= 2);
    let sid = SessionId::new(1);
    // SAFETY: this replay owns an isolated resource and its only issuer.
    let network = InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
    let [mut slab0, mut slab1, mut slab2] = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
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
    macro_rules! register {
        ($id:literal) => {
            if resolver_count > $id {
                let resolver = ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id)));
                if programs.owner == 0 {
                    rv0.set_resolver(&programs.p0, resolver).unwrap();
                } else {
                    rv1.set_resolver(&programs.p1, resolver).unwrap();
                }
            }
        };
    }
    register_resolvers!();
    let mut e0 = rv0.enter(sid, &programs.p0).unwrap();
    let mut e1 = rv1.enter(sid, &programs.p1).unwrap();
    let mut e2 = programs
        .third_actor
        .then(|| rv2.enter(sid, &programs.p2).unwrap());
    let mut resource = Some(if programs.owner == 0 {
        rv0.enter(sid, &programs.p3).unwrap()
    } else {
        rv1.enter(sid, &programs.p3).unwrap()
    });

    let present = groups
        .iter()
        .map(|labels| {
            core::array::from_fn::<_, 3, _>(|role| {
                messages.iter().any(|m| {
                    labels.contains(&m.label)
                        && (usize::from(m.from) == role || usize::from(m.to) == role)
                })
            })
        })
        .collect::<Vec<_>>();
    let count = |role: usize| present.iter().filter(|roles| roles[role]).count();
    let mut a = fragments(&mut e0, count(0)).into_iter();
    let mut b = fragments(&mut e1, count(1)).into_iter();
    let mut c = match e2.as_mut() {
        Some(endpoint) => fragments(endpoint, count(2)),
        None => {
            assert_eq!(count(2), 0);
            Vec::new()
        }
    }
    .into_iter();
    let observed = PendingCalls::default();
    let mut work = Vec::new();
    for (fragment, (labels, roles)) in groups.iter().zip(present).enumerate() {
        let a = roles[0].then(|| a.next().unwrap());
        let b = roles[1].then(|| b.next().unwrap());
        let c = roles[2].then(|| c.next().unwrap());
        let loan = if messages
            .iter()
            .any(|m| m.owned && labels.contains(&m.label))
        {
            Some(resource.take().expect("multiple resource fragments"))
        } else {
            None
        };
        let commands = commands
            .iter()
            .filter(|c| labels.contains(&c.message.label))
            .copied()
            .collect::<Vec<_>>();
        let scopes = messages
            .iter()
            .filter(|m| labels.contains(&m.label))
            .flat_map(|m| m.decisions.iter().map(|(id, _)| *id))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let observed = &observed;
        let (initial0, initial1) = if programs.owner == 0 {
            (loan, None)
        } else {
            (None, loan)
        };
        work.push(async move {
            let (returned0, returned1, service) = tokio::join!(
                drive_participant::<0>(
                    a,
                    initial0,
                    &commands,
                    (decisions, &scopes, programs.owner),
                    observed,
                    fragment
                ),
                drive_participant::<1>(
                    b,
                    initial1,
                    &commands,
                    (decisions, &scopes, programs.owner),
                    observed,
                    fragment
                ),
                drive_participant::<2>(
                    c,
                    None,
                    &commands,
                    (decisions, &scopes, programs.owner),
                    observed,
                    fragment
                ),
            );
            assert!(service.is_none());
            if programs.owner == 0 {
                assert!(returned1.is_none());
                returned0
            } else {
                assert!(returned0.is_none());
                returned1
            }
        });
    }
    assert!(a.next().is_none() && b.next().is_none() && c.next().is_none());
    assert!(resource.is_none());
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        let width = work.len();
        let mut work = work.into_iter();
        let returned = match width {
            2 => join_widths!(),
            _ => panic!("concurrent width outside source inventory"),
        };
        assert!(work.next().is_none());
        assert_eq!(returned.iter().filter(|loan| loan.is_some()).count(), 1);
    })
    .await
    .expect("independent projected fragments stalled");
    assert!(observed.calls.borrow().is_empty());
    assert!(
        observed.overlapped.get(),
        "independent borrows never overlapped while pending"
    );
}
