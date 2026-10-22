// Observe actual I/O overlap and completion without choosing protocol steps.
#[derive(Default)]
struct PendingCalls {
    calls: std::cell::RefCell<std::collections::BTreeSet<ObservedCall>>,
    completed: std::cell::RefCell<Vec<(u8, u8, bool)>>,
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
    fn json(self, script: usize, concurrent: bool) -> String {
        format!(
            r#"{{"script":{script},"concurrent":{concurrent},"group":[{},{}],"part":{},"occurrence":{},"actor":{},"role":{},"label":{},"receive":{},"accepted":false}}"#,
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
fn load_scope_plans(text: &str, name: usize, scripts: usize) -> Vec<Vec<ScopeProbe>> {
    use planned_json::Json;
    fn number(value: &Json) -> usize {
        match value {
            Json::Number(value) => value.parse().expect("invalid scope-plan number"),
            _ => panic!("expected scope-plan number"),
        }
    }
    fn field(record: &Json, key: &str) -> usize {
        number(record.get(key).expect("missing scope-plan field"))
    }
    fn boolean(record: &Json, key: &str) -> bool {
        match record.get(key) {
            Some(Json::Bool(value)) => *value,
            _ => panic!("expected scope-plan boolean"),
        }
    }
    let input = planned_json::parse(text).expect("invalid scope-plan JSON");
    assert_eq!(field(&input, "name"), name, "wrong scope-plan case");
    let Json::Object(fields) = &input else {
        panic!("expected scope-plan object")
    };
    assert_eq!(fields.len(), 2, "unknown scope-plan field");
    let Some(Json::Array(records)) = input.get("probes") else {
        panic!("missing scope-plan probes")
    };
    let mut plans = (0..scripts).map(|_| Vec::new()).collect::<Vec<_>>();
    for record in records {
        let Json::Object(fields) = record else {
            panic!("expected scope probe object")
        };
        assert_eq!(fields.len(), 10, "unknown scope probe field");
        assert!(
            !boolean(record, "accepted"),
            "a scope plan must request rejection"
        );
        let Some(Json::Array(group)) = record.get("group") else {
            panic!("missing scope group")
        };
        assert_eq!(group.len(), 2);
        let probe = ScopeProbe {
            group: (
                number(&group[0]).try_into().unwrap(),
                number(&group[1]).try_into().unwrap(),
            ),
            part: field(record, "part").try_into().unwrap(),
            occurrence: field(record, "occurrence"),
            actor: field(record, "actor").try_into().unwrap(),
            role: field(record, "role").try_into().unwrap(),
            label: field(record, "label").try_into().unwrap(),
            receive: boolean(record, "receive"),
        };
        let script = field(record, "script");
        let plan = plans
            .get_mut(script)
            .expect("scope-plan script outside inventory");
        if !boolean(record, "concurrent") {
            plan.push(probe);
        }
    }
    plans
}

#[test]
fn scope_plan_loading_keeps_each_source_visit_and_both_schedules() {
    let text = r#"{"name":7,"probes":[
      {"script":0,"concurrent":false,"group":[1,2],"part":1,"occurrence":2,"actor":0,"role":0,"label":2,"receive":false,"accepted":false},
      {"script":0,"concurrent":true,"group":[1,2],"part":1,"occurrence":2,"actor":0,"role":0,"label":2,"receive":false,"accepted":false},
      {"script":1,"concurrent":false,"group":[1,2],"part":1,"occurrence":4,"actor":1,"role":3,"label":2,"receive":true,"accepted":false},
      {"script":1,"concurrent":true,"group":[1,2],"part":1,"occurrence":4,"actor":1,"role":3,"label":2,"receive":true,"accepted":false}]}"#;
    let plans = load_scope_plans(text, 7, 2);
    assert_eq!(plans.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(plans[0][0].occurrence, 2);
    assert_eq!(plans[1][0].occurrence, 4);
    assert_eq!(plans[1][0].role, 3);
    assert!(std::panic::catch_unwind(|| load_scope_plans(text, 8, 2)).is_err());
    assert!(std::panic::catch_unwind(|| load_scope_plans(text, 7, 1)).is_err());
    assert!(
        std::panic::catch_unwind(|| load_scope_plans(
            &text.replace("\"accepted\":false", "\"accepted\":true"),
            7,
            2
        ))
        .is_err()
    );
    assert!(std::panic::catch_unwind(|| load_scope_plans(&text[..text.len() - 1], 7, 2)).is_err());
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ObservedCall {
    Message(u8, u8, bool),
    Preview(u8, u8),
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
            ObservedCall::Message(0, 2, false)
        ),
    )
    .unwrap();
    assert!(observed.overlapped.get());
    assert!(observed.calls.borrow().is_empty());
    assert_eq!(*observed.completed.borrow(), vec![(0, 2, false)]);
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
                && let ObservedCall::Message(actor, label, receive) = caller
            {
                observed
                    .completed
                    .borrow_mut()
                    .push((actor, label, receive));
            }
            core::task::Poll::Ready(output)
        }
    })
    .await
}
