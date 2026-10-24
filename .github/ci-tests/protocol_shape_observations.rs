// Observe actual I/O overlap and completion without choosing protocol steps.
#[derive(Default)]
struct PendingCalls {
    calls: std::cell::RefCell<std::collections::BTreeSet<ObservedCall>>,
    completed: std::cell::RefCell<Vec<(u8, u8, u8, bool)>>,
    overlapped: std::cell::Cell<bool>,
    scope_rejections: std::cell::RefCell<Vec<ScopeProbe>>,
}

// Immutable test coordinates. An occurrence identifies a source visit, even
// when earlier visits used the same labels and made the same branch choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeProbe {
    group: (u8, u8),
    part: u8,
    occurrence: usize,
    actor: u8,
    role: u8,
    label: u8,
    receive: bool,
}
impl ScopeProbe {
    fn json(self) -> String {
        format!(
            r#"{{"group":[{},{}],"part":{},"occurrence":{},"actor":{},"role":{},"label":{},"receive":{},"accepted":false}}"#,
            self.group.0,
            self.group.1,
            self.part,
            self.occurrence,
            self.actor,
            self.role,
            self.label,
            self.receive
        )
    }
}

// Schedule data stays outside Rust's const/type evaluation. Its complete
// source-derived inventory is checked separately against observed calls.
struct ScopePlans<R: std::io::BufRead> {
    lines: std::io::Lines<R>,
    next: usize,
    scripts: usize,
}
impl<R: std::io::BufRead> ScopePlans<R> {
    fn number(value: &planned_json::Json) -> usize {
        match value {
            planned_json::Json::Number(value) => value.parse().expect("invalid scope-plan number"),
            _ => panic!("expected scope-plan number"),
        }
    }
    fn field(record: &planned_json::Json, key: &str) -> usize {
        Self::number(record.get(key).expect("missing scope-plan field"))
    }
    fn boolean(record: &planned_json::Json, key: &str) -> bool {
        match record.get(key) {
            Some(planned_json::Json::Bool(value)) => *value,
            _ => panic!("expected scope-plan boolean"),
        }
    }
    fn read(lines: &mut std::io::Lines<R>) -> planned_json::Json {
        planned_json::parse(
            &lines
                .next()
                .expect("missing scope-plan record")
                .expect("unreadable scope-plan record"),
        )
        .expect("invalid scope-plan JSON")
    }
    fn new(reader: R, name: usize, scripts: usize) -> Self {
        let mut lines = reader.lines();
        let header = Self::read(&mut lines);
        let planned_json::Json::Object(fields) = &header else {
            panic!("expected scope-plan header")
        };
        assert_eq!(fields.len(), 2, "unknown scope-plan header field");
        assert_eq!(Self::field(&header, "name"), name, "wrong scope-plan case");
        assert_eq!(
            Self::field(&header, "scripts"),
            scripts,
            "wrong scope-plan count"
        );
        Self {
            lines,
            next: 0,
            scripts,
        }
    }
    fn next(&mut self, script: usize) -> Vec<ScopeProbe> {
        use planned_json::Json;
        assert_eq!(self.next, script, "out-of-order scope-plan script");
        assert!(script < self.scripts, "unexpected extra script plan");
        let input = Self::read(&mut self.lines);
        let Json::Object(fields) = &input else {
            panic!("expected scope-plan object")
        };
        assert_eq!(fields.len(), 2, "unknown scope-plan field");
        assert_eq!(
            Self::field(&input, "script"),
            script,
            "wrong scope-plan script"
        );
        let Some(Json::Array(records)) = input.get("probes") else {
            panic!("missing scope-plan probes")
        };
        let probes = records
            .iter()
            .map(|record| {
                let Json::Object(fields) = record else {
                    panic!("expected scope probe object")
                };
                assert_eq!(fields.len(), 8, "unknown scope probe field");
                assert!(
                    !Self::boolean(record, "accepted"),
                    "a scope plan must request rejection"
                );
                let Some(Json::Array(group)) = record.get("group") else {
                    panic!("missing scope group")
                };
                assert_eq!(group.len(), 2);
                ScopeProbe {
                    group: (
                        Self::number(&group[0]).try_into().unwrap(),
                        Self::number(&group[1]).try_into().unwrap(),
                    ),
                    part: Self::field(record, "part").try_into().unwrap(),
                    occurrence: Self::field(record, "occurrence"),
                    actor: Self::field(record, "actor").try_into().unwrap(),
                    role: Self::field(record, "role").try_into().unwrap(),
                    label: Self::field(record, "label").try_into().unwrap(),
                    receive: Self::boolean(record, "receive"),
                }
            })
            .collect();
        self.next += 1;
        probes
    }
    fn finish(mut self) {
        assert_eq!(self.next, self.scripts, "missing scope-plan script");
        assert!(
            self.lines.next().is_none(),
            "unexpected extra scope-plan record"
        );
    }
}

#[test]
fn scope_plan_loading_keeps_each_source_visit_and_both_schedules() {
    let text = concat!(
        "{\"name\":7,\"scripts\":2}\n",
        "{\"script\":0,\"probes\":[{\"group\":[1,2],\"part\":1,\"occurrence\":2,\"actor\":0,\"role\":0,\"label\":2,\"receive\":false,\"accepted\":false}]}\n",
        "{\"script\":1,\"probes\":[{\"group\":[1,2],\"part\":1,\"occurrence\":4,\"actor\":1,\"role\":3,\"label\":2,\"receive\":true,\"accepted\":false}]}\n",
    );
    let load = |text: &str, name, scripts| {
        let mut input = ScopePlans::new(text.as_bytes(), name, scripts);
        let plans = (0..scripts)
            .map(|script| input.next(script))
            .collect::<Vec<_>>();
        input.finish();
        plans
    };
    let plans = load(text, 7, 2);
    assert_eq!(plans.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(plans[0][0].occurrence, 2);
    assert_eq!(plans[1][0].occurrence, 4);
    assert_eq!(plans[1][0].role, 3);
    assert!(std::panic::catch_unwind(|| load(text, 8, 2)).is_err());
    assert!(std::panic::catch_unwind(|| load(text, 7, 1)).is_err());
    assert!(
        std::panic::catch_unwind(|| load(
            &text.replace("\"accepted\":false", "\"accepted\":true"),
            7,
            2
        ))
        .is_err()
    );
    for changed in [
        text[..text.len() - 2].to_owned(),
        text.replace("\"script\":1", "\"script\":0"),
        format!("{text}{{}}\n"),
        text.lines().take(2).collect::<Vec<_>>().join("\n"),
    ] {
        assert!(std::panic::catch_unwind(|| load(&changed, 7, 2)).is_err());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ObservedCall {
    Message(u8, u8, u8, bool),
    Preview(u8, u8),
}

// Read the logical role from the actual Endpoint type, including par borrows.
trait ObservedEndpointRole {
    const ROLE: u8;
}
impl<const ROLE: u8> ObservedEndpointRole for Endpoint<'_, ROLE> {
    const ROLE: u8 = ROLE;
}
impl<const ROLE: u8> ObservedEndpointRole for hibana::ParallelEndpoint<'_, '_, ROLE> {
    const ROLE: u8 = ROLE;
}
fn observation_role<E: ObservedEndpointRole>(_: &E) -> u8 {
    E::ROLE
}
impl ObservedCall {
    fn actor(self) -> u8 {
        match self {
            Self::Message(actor, ..) | Self::Preview(actor, _) => actor,
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_pending_preview_counts_as_work_but_not_as_a_completed_message() {
    let observed = PendingCalls::default();
    tokio::try_join!(
        record_pending(
            async {
                tokio::task::yield_now().await;
                Ok(())
            },
            &observed,
            ObservedCall::Preview(0, 1)
        ),
        record_pending(
            async {
                tokio::task::yield_now().await;
                Ok(())
            },
            &observed,
            ObservedCall::Message(0, 0, 2, false)
        ),
    )
    .unwrap();
    assert!(observed.overlapped.get());
    assert!(observed.calls.borrow().is_empty());
    assert_eq!(*observed.completed.borrow(), vec![(0, 0, 2, false)]);
}

#[tokio::test(flavor = "current_thread")]
async fn actual_parallel_receives_can_both_wait_and_then_complete() {
    type Left = g::Msg<0, u32>;
    type Right = g::Msg<1, u32>;
    let global = g::par(g::send::<0, 1, Left>(), g::send::<2, 1, Right>());
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let p2 = project::<2>(&global);
    let network = InMemoryTransport::new(None);
    let [mut slab0, mut slab1, mut slab2] = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
    let mut s0 = SessionKitStorage::uninit();
    let mut s1 = SessionKitStorage::uninit();
    let mut s2 = SessionKitStorage::uninit();
    let rv0 = s0.init().rendezvous(&mut slab0, network.clone()).unwrap();
    let rv1 = s1.init().rendezvous(&mut slab1, network.clone()).unwrap();
    let rv2 = s2.init().rendezvous(&mut slab2, network).unwrap();
    let sid = SessionId::new(8);
    let mut sender0 = rv0.enter(sid, &p0).unwrap();
    let mut receiver = rv1.enter(sid, &p1).unwrap();
    let mut sender2 = rv2.enter(sid, &p2).unwrap();
    let observed = &PendingCalls::default();
    let receiving = async {
        let [mut left, mut right] = receiver.par()?;
        tokio::try_join!(
            record_pending(
                left.recv::<Left>(),
                observed,
                ObservedCall::Message(1, 1, 0, true)
            ),
            record_pending(
                right.recv::<Right>(),
                observed,
                ObservedCall::Message(1, 1, 1, true)
            ),
        )
    };
    let sending0 = async {
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        sender0.send::<Left>(&7).await
    };
    let sending2 = async {
        tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        sender2.send::<Right>(&9).await
    };
    let ((left, right), (), ()) = tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(receiving, sending0, sending2)
    })
    .await
    .expect("parallel receives stalled")
    .unwrap();
    assert_eq!((left, right), (7, 9));
    assert!(
        observed.overlapped.get(),
        "the two real receives never overlapped while pending"
    );
    assert!(observed.calls.borrow().is_empty());
    assert_eq!(observed.completed.borrow().len(), 2);
}

async fn record_pending<F, T>(
    future: F,
    observed: &PendingCalls,
    caller: ObservedCall,
) -> Result<T, hibana::EndpointError>
where
    F: core::future::Future<Output = Result<T, hibana::EndpointError>>,
{
    tokio::pin!(future);
    core::future::poll_fn(|cx| match future.as_mut().poll(cx) {
        core::task::Poll::Pending => {
            let mut calls = observed.calls.borrow_mut();
            calls.insert(caller);
            if calls
                .iter()
                .any(|other| other.actor() == caller.actor() && *other != caller)
            {
                observed.overlapped.set(true);
            }
            core::task::Poll::Pending
        }
        core::task::Poll::Ready(output) => {
            observed.calls.borrow_mut().remove(&caller);
            if output.is_ok()
                && let ObservedCall::Message(actor, role, label, receive) = caller
            {
                observed
                    .completed
                    .borrow_mut()
                    .push((actor, role, label, receive));
            }
            core::task::Poll::Ready(output)
        }
    })
    .await
}
