//! Wake delivery and virtual I/O time for closed in-process test transports.
use core::{
    cell::Cell,
    future::{Future, poll_fn},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Poll, RawWaker, RawWakerVTable, Waker},
    time::Duration,
};

static WAKES: AtomicUsize = AtomicUsize::new(0);

// Time is test I/O data, isolated per thread. The driver advances it only once
// runnable work has parked. A cancelled timer may leave an earlier empty tick;
// it cannot advance past another pending timer's registered deadline.
std::thread_local! {
    static NOW: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    static NEXT: Cell<Option<Duration>> = const { Cell::new(None) };
}

pub fn now() -> Duration {
    NOW.with(Cell::get)
}

pub async fn sleep(duration: Duration) {
    let deadline = now().checked_add(duration).expect("test time overflow");
    poll_fn(|_| {
        if now() >= deadline {
            return Poll::Ready(());
        }
        NEXT.with(|next| next.set(Some(next.get().map_or(deadline, |at| at.min(deadline)))));
        Poll::Pending
    })
    .await;
}

pub async fn timeout<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, std::io::Error> {
    hibana_runtime::race(async { Ok(future.await) }, async {
        sleep(duration).await;
        Err(std::io::ErrorKind::TimedOut.into())
    })
    .await
}

pub struct InProcess {
    observed: usize,
}

impl InProcess {
    fn generation() -> usize {
        WAKES.load(Ordering::Acquire)
    }
}

impl Default for InProcess {
    fn default() -> Self {
        NOW.with(|now| now.set(Duration::ZERO));
        NEXT.with(|next| next.set(None));
        Self {
            observed: Self::generation(),
        }
    }
}

impl hibana_runtime::Driver for InProcess {
    fn waker(&self) -> Waker {
        // SAFETY: callbacks only access the static counter. Clones retain no
        // address into this driver or a future and remain valid on any thread.
        unsafe { Waker::from_raw(raw()) }
    }

    fn wait(&mut self) {
        let current = Self::generation();
        if current != self.observed {
            self.observed = current;
            return;
        }
        let next = NEXT
            .with(Cell::take)
            .expect("closed in-process execution stalled");
        NOW.with(|now| now.set(now.get().max(next)));
    }
}

fn raw() -> RawWaker {
    RawWaker::new(ptr::null(), &VTABLE)
}
fn clone(_: *const ()) -> RawWaker {
    raw()
}
fn wake(_: *const ()) {
    WAKES.fetch_add(1, Ordering::Release);
}
fn drop_waker(_: *const ()) {}
static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, drop_waker);
