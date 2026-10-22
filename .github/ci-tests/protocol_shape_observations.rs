// Observe actual I/O overlap and completion without choosing protocol steps.
#[derive(Default)]
struct PendingCalls {
    calls: std::cell::RefCell<std::collections::BTreeSet<ObservedCall>>,
    completed: std::cell::RefCell<Vec<(u8, u8, bool)>>,
    overlapped: std::cell::Cell<bool>,
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
