use super::*;
use core::cell::UnsafeCell;

#[derive(Default)]
struct TestLog {
    entries: [Option<&'static str>; 8],
    len: usize,
}

impl TestLog {
    fn push(&mut self, label: &'static str) {
        assert!(self.len < self.entries.len(), "test log capacity exceeded");
        self.entries[self.len] = Some(label);
        self.len += 1;
    }

    fn as_slice(&self) -> [&'static str; 2] {
        let mut out = [""; 2];
        let mut idx = 0usize;
        while idx < out.len() && idx < self.len {
            out[idx] = self.entries[idx].expect("occupied test log entry");
            idx += 1;
        }
        out
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

struct TestLogCell {
    log: UnsafeCell<TestLog>,
}

impl TestLogCell {
    fn new() -> Self {
        Self {
            log: UnsafeCell::new(TestLog::default()),
        }
    }

    fn push(&self, label: &'static str) {
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe { (&mut *self.log.get()).push(label) }
    }

    fn as_slice(&self) -> [&'static str; 2] {
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { (&*self.log.get()).as_slice() }
    }

    fn is_empty(&self) -> bool {
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { (&*self.log.get()).is_empty() }
    }
}

#[derive(Clone, Copy, Default)]
struct TestFacet;

struct TestContext<'ctx> {
    log: &'ctx TestLogCell,
    label: &'static str,
    value: u32,
}

impl<'ctx> TestContext<'ctx> {
    fn new(log: &'ctx TestLogCell, label: &'static str, value: u32) -> Self {
        Self { log, label, value }
    }
}

impl LeaseFacet for TestFacet {
    type Context<'ctx> = TestContext<'ctx>;

    fn on_commit<'ctx>(&self, context: &mut Self::Context<'ctx>) {
        context.log.push(context.label);
    }

    fn on_rollback<'ctx>(&self, context: &mut Self::Context<'ctx>) {
        context.log.push(context.label);
    }
}

struct TestSpec;
impl LeaseSpec for TestSpec {
    type NodeId = u8;
    type Facet = TestFacet;
    type ChildStorage = InlineLeaseChildStorage<u8, 3>;
    type NodeStorage<'graph>
        = InlineLeaseNodeStorage<'graph, Self, 4>
    where
        Self: 'graph;
    const MAX_NODES: usize = 4;
    const MAX_CHILDREN: usize = 3;
}

fn test_graph<'ctx>(
    root_id: u8,
    log: &'ctx TestLogCell,
    label: &'static str,
    value: u32,
) -> LeaseGraph<'ctx, TestSpec> {
    let mut graph = MaybeUninit::<LeaseGraph<'ctx, TestSpec>>::uninit();
    /* SAFETY: the caller supplies exclusive uninitialized storage and this initializer writes all exposed fields before return. */
    unsafe {
        LeaseGraph::<TestSpec>::init_new(
            graph.as_mut_ptr(),
            root_id,
            TestFacet,
            TestContext::new(log, label, value),
        );
        graph.assume_init()
    }
}

#[test]
fn handle_updates_context() {
    let log = TestLogCell::new();
    let mut graph = test_graph(0, &log, "root", 1);
    graph
        .add_child(0, 1, TestFacet, TestContext::new(&log, "child", 2))
        .unwrap();

    {
        let mut handle = graph.handle_mut(1).unwrap();
        handle.context().value = 42;
    }

    assert_eq!(graph.handle_mut(1).unwrap().context().value, 42);
    assert!(log.is_empty());
}

#[test]
fn commit_traverses_in_reverse_order() {
    let log = TestLogCell::new();
    let mut graph = test_graph(0, &log, "commit_root", 0);
    graph
        .add_child(0, 1, TestFacet, TestContext::new(&log, "commit_child", 0))
        .unwrap();

    graph.commit();

    assert_eq!(log.as_slice(), ["commit_child", "commit_root"]);
}

#[test]
fn rollback_traverses_in_reverse_order() {
    let log = TestLogCell::new();
    let mut graph = test_graph(0, &log, "rollback_root", 0);
    graph
        .add_child(0, 1, TestFacet, TestContext::new(&log, "rollback_child", 0))
        .unwrap();

    graph.rollback();

    assert_eq!(log.as_slice(), ["rollback_child", "rollback_root"]);
}

#[test]
fn navigate_accesses_descendants() {
    let log = TestLogCell::new();
    let mut graph = test_graph(0, &log, "root", 1);
    graph
        .add_child(0, 1, TestFacet, TestContext::new(&log, "child", 2))
        .unwrap();

    let mut handle = graph.navigate_mut(&[0, 1]).unwrap();
    handle.context().value = 7;

    assert_eq!(graph.handle_mut(1).unwrap().context().value, 7);
    assert!(graph.navigate_mut(&[0, 2]).is_none());
}

#[test]
fn children_iterator_exposes_inserted_ids() {
    let log = TestLogCell::new();
    let mut graph = test_graph(5, &log, "root", 0);

    graph
        .add_child(5, 7, TestFacet, TestContext::new(&log, "child_a", 0))
        .unwrap();
    graph
        .add_child(5, 9, TestFacet, TestContext::new(&log, "child_b", 0))
        .unwrap();

    let mut iter = graph.children(5).expect("root present");
    assert_eq!(iter.next(), Some(7));
    assert_eq!(iter.next(), Some(9));
    assert_eq!(iter.next(), None);
}
