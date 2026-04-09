//! LeaseGraph — type-driven ownership tracking.
//!
//! When multiple leases form a parent/child hierarchy (delegation, splice, etc.)
//! this data structure guarantees that:
//! - ownership flows from the root to its children in a single direction;
//! - every lease reaches exactly one commit or rollback; and
//! - drop order follows reverse topological order (children are dropped before parents).
//!
//! ## no_alloc friendly
//!
//! To avoid recursive types, `LeaseGraph` stores every node in a flat array and
//! keeps child IDs in per-node arrays. No `Box` or `BTreeMap` is required.
//!
//! ## Facet API integration
//!
//! A `LeaseGraph` can carry arbitrary facet markers, making it a natural fit for
//! rendezvous lease management. Delegation stores child rendezvous capability
//! leases, while splice stores child splice leases.
//!
//! ```rust,ignore
//! use hibana::substrate::RendezvousId;
//! use hibana::control::lease::graph::{LeaseGraph, LeaseSpec};
//!
//! // Example: rendezvous IDs with a minimal unit facet.
//! struct RvSlotSpec;
//! impl LeaseSpec for RvSlotSpec {
//!     type NodeId = RendezvousId;
//!     type Facet = ();
//!     const MAX_NODES: usize = 8;
//!     const MAX_CHILDREN: usize = 4;
//! }
//!
//! // Acquire the root rendezvous slot lease
//! let mut graph = LeaseGraph::<RvSlotSpec>::new(
//!     root_id,
//!     (),
//!     root_context,
//! );
//!
//! // Add a child rendezvous (with context)
//! graph.add_child(
//!     root_id,
//!     child_id,
//!     SlotFacet::default(),
//!     child_context,
//! ).unwrap();
//!
//! // Commit drops the nodes in reverse topological order (child → parent)
//! graph.commit();
//! ```

use core::mem::MaybeUninit;

/// LeaseFacet is a zero-sized marker that carries behaviour for commit/rollback
/// while delegating state storage to an explicit context object.
pub(crate) trait LeaseFacet: Copy + Default {
    /// Per-node context associated with the facet.
    type Context<'ctx>;

    /// Called during `LeaseGraph::commit` once all children have been committed.
    fn on_commit<'ctx>(&self, context: &mut Self::Context<'ctx>);

    /// Called during `LeaseGraph::rollback` once all children have been rolled back.
    fn on_rollback<'ctx>(&self, context: &mut Self::Context<'ctx>);
}

/// Fixed-capacity child storage used by a [`LeaseSpec`].
pub(crate) trait LeaseChildStorage<Id: Copy>: Copy {
    const CAPACITY: usize;

    fn empty() -> Self;

    fn get(&self, idx: usize) -> Option<Id>;

    fn set(&mut self, idx: usize, id: Id);
}

#[derive(Clone, Copy)]
pub(crate) struct InlineLeaseChildStorage<Id: Copy + Default, const CAPACITY: usize> {
    slots: [Id; CAPACITY],
}

impl<Id: Copy + Default, const CAPACITY: usize> LeaseChildStorage<Id>
    for InlineLeaseChildStorage<Id, CAPACITY>
{
    const CAPACITY: usize = CAPACITY;

    #[inline]
    fn empty() -> Self {
        Self {
            slots: [Id::default(); CAPACITY],
        }
    }

    #[inline]
    fn get(&self, idx: usize) -> Option<Id> {
        self.slots.get(idx).copied()
    }

    #[inline]
    fn set(&mut self, idx: usize, id: Id) {
        self.slots[idx] = id;
    }
}

/// LeaseSpec defines the node identifier and facet used by a LeaseGraph.
pub(crate) trait LeaseSpec: Sized {
    /// Node identifier (e.g. RendezvousId)
    type NodeId: Copy + Eq + Ord + core::fmt::Debug;

    /// Facet marker associated with each node.
    type Facet: LeaseFacet;

    /// Fixed-capacity child storage for each node in the graph.
    type ChildStorage: LeaseChildStorage<Self::NodeId>;

    /// Fixed-capacity node storage backing the graph itself.
    type NodeStorage<'graph>: LeaseNodeStorage<'graph, Self>
    where
        Self: 'graph;

    /// Maximum number of nodes (including the root) supported by this graph.
    const MAX_NODES: usize;

    /// Maximum number of children each node may own.
    const MAX_CHILDREN: usize;
}

impl LeaseFacet for () {
    type Context<'ctx> = ();

    fn on_commit<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}

    fn on_rollback<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}
}

/// Global state of the lease graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphState {
    Active,
    Committed,
    RolledBack,
}

/// Internal node payload stored inside the flat array.
pub(crate) struct NodeData<'graph, S: LeaseSpec> {
    id: S::NodeId,
    facet: S::Facet,
    context: <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
    /// Array of child node identifiers.
    children: S::ChildStorage,
    child_count: usize,
}

impl<'graph, S: LeaseSpec> NodeData<'graph, S> {
    fn new(
        id: S::NodeId,
        facet: S::Facet,
        context: <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
    ) -> Self {
        Self {
            id,
            facet,
            context,
            children: S::ChildStorage::empty(),
            child_count: 0,
        }
    }

    fn add_child(&mut self, child_id: S::NodeId) -> Result<(), LeaseGraphError> {
        if self.child_count >= S::MAX_CHILDREN || self.child_count >= S::ChildStorage::CAPACITY {
            return Err(LeaseGraphError::TooManyChildren);
        }
        self.children.set(self.child_count, child_id);
        self.child_count += 1;
        Ok(())
    }
}

/// Fixed-capacity node storage used by a [`LeaseSpec`].
pub(crate) trait LeaseNodeStorage<'graph, S: LeaseSpec> {
    const CAPACITY: usize;

    #[cfg(test)]
    fn empty() -> Self;
    unsafe fn init_empty(dst: *mut Self);

    unsafe fn write(&mut self, idx: usize, node: NodeData<'graph, S>);

    unsafe fn read(&mut self, idx: usize) -> NodeData<'graph, S>;

    fn get(&self, idx: usize) -> Option<&NodeData<'graph, S>>;

    fn get_mut(&mut self, idx: usize) -> Option<&mut NodeData<'graph, S>>;
}

pub(crate) struct InlineLeaseNodeStorage<'graph, S: LeaseSpec, const CAPACITY: usize> {
    slots: [MaybeUninit<NodeData<'graph, S>>; CAPACITY],
}

impl<'graph, S: LeaseSpec, const CAPACITY: usize> LeaseNodeStorage<'graph, S>
    for InlineLeaseNodeStorage<'graph, S, CAPACITY>
{
    const CAPACITY: usize = CAPACITY;

    #[inline]
    #[cfg(test)]
    fn empty() -> Self {
        let mut storage = MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_empty(storage.as_mut_ptr());
            storage.assume_init()
        }
    }

    #[inline]
    unsafe fn init_empty(dst: *mut Self) {
        unsafe {
            let slots =
                core::ptr::addr_of_mut!((*dst).slots).cast::<MaybeUninit<NodeData<'graph, S>>>();
            let mut i = 0;
            while i < CAPACITY {
                slots.add(i).write(MaybeUninit::uninit());
                i += 1;
            }
        }
    }

    #[inline]
    unsafe fn write(&mut self, idx: usize, node: NodeData<'graph, S>) {
        debug_assert!(idx < CAPACITY, "lease graph node index out of bounds");
        unsafe {
            self.slots.get_unchecked_mut(idx).write(node);
        }
    }

    #[inline]
    unsafe fn read(&mut self, idx: usize) -> NodeData<'graph, S> {
        debug_assert!(idx < CAPACITY, "lease graph node index out of bounds");
        unsafe { self.slots.get_unchecked(idx).assume_init_read() }
    }

    #[inline]
    fn get(&self, idx: usize) -> Option<&NodeData<'graph, S>> {
        if idx >= CAPACITY {
            return None;
        }
        Some(unsafe { self.slots.get_unchecked(idx).assume_init_ref() })
    }

    #[inline]
    fn get_mut(&mut self, idx: usize) -> Option<&mut NodeData<'graph, S>> {
        if idx >= CAPACITY {
            return None;
        }
        Some(unsafe { self.slots.get_unchecked_mut(idx).assume_init_mut() })
    }
}

/// Iterator over a node's direct children.
#[cfg(test)]
pub(crate) struct ChildIter<'a, 'graph, S: LeaseSpec> {
    node: &'a NodeData<'graph, S>,
    index: usize,
}

#[cfg(test)]
impl<'a, 'graph, S: LeaseSpec> ChildIter<'a, 'graph, S> {
    fn new(node: &'a NodeData<'graph, S>) -> Self {
        Self { node, index: 0 }
    }
}

#[cfg(test)]
impl<'a, 'graph, S: LeaseSpec> Iterator for ChildIter<'a, 'graph, S> {
    type Item = S::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.node.child_count {
            let slot = self.index;
            self.index += 1;
            if let Some(id) = self.node.children.get(slot) {
                return Some(id);
            }
        }
        None
    }
}

/// Mutable handle exposing a facet marker and its associated context.
pub(crate) struct FacetHandle<'a, 'graph, S: LeaseSpec> {
    facet: S::Facet,
    context: &'a mut <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
}

impl<'a, 'graph, S: LeaseSpec> FacetHandle<'a, 'graph, S> {
    #[inline]
    pub(crate) fn context(
        &mut self,
    ) -> &mut <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph> {
        let _ = self.facet;
        self.context
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn with<R>(
        self,
        f: impl FnOnce(S::Facet, &mut <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>) -> R,
    ) -> R {
        let FacetHandle { facet, context } = self;
        f(facet, context)
    }
}

/// Errors emitted by `LeaseGraph` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseGraphError {
    /// Graph capacity exceeded.
    GraphFull,
    /// Child capacity per node exceeded.
    TooManyChildren,
    /// Requested node ID not found.
    NodeNotFound,
    /// Duplicate node identifier.
    DuplicateId,
    /// Graph has already been committed.
    AlreadyCommitted,
    /// Graph has already been rolled back.
    AlreadyRolledBack,
}

/// LeaseGraph stores parent/child relations in a flat array.
///
/// ## Layout
///
/// - `nodes`: fixed-size storage that holds every node.
/// - each `NodeData` entry keeps an array of child identifiers.
/// - `root_id`: identifier of the root node.
/// - `state`: aggregated graph state (Active / Committed / RolledBack).
///
/// ## Typestate
///
/// - `commit(&mut self)` / `rollback(&mut self)` move the graph into a terminal
///   state; subsequent stateful operations are rejected.
pub(crate) struct LeaseGraph<'graph, S: LeaseSpec + 'graph> {
    /// Storage backing every node.
    nodes: S::NodeStorage<'graph>,
    /// Number of nodes currently present.
    node_count: usize,
    /// Identifier of the root node.
    root_id: S::NodeId,
    /// Current state of the graph.
    state: GraphState,
}

impl<'graph, S: LeaseSpec + 'graph> LeaseGraph<'graph, S> {
    /// Initialize a new `LeaseGraph` directly into destination storage.
    ///
    /// # Safety
    /// `dst` must point to valid, writable memory for `Self`.
    pub(crate) unsafe fn init_new(
        dst: *mut Self,
        root_id: S::NodeId,
        root_facet: S::Facet,
        root_context: <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
    ) {
        debug_assert!(S::MAX_NODES > 0, "LeaseGraph requires MAX_NODES > 0");
        debug_assert!(S::MAX_CHILDREN > 0, "LeaseGraph requires MAX_CHILDREN > 0");
        debug_assert!(
            S::MAX_NODES == <S::NodeStorage<'graph> as LeaseNodeStorage<'graph, S>>::CAPACITY,
            "LeaseGraph node storage must match LeaseSpec capacity"
        );
        debug_assert!(
            S::MAX_CHILDREN == <S::ChildStorage as LeaseChildStorage<S::NodeId>>::CAPACITY,
            "LeaseGraph child storage must match LeaseSpec capacity"
        );

        unsafe {
            S::NodeStorage::init_empty(core::ptr::addr_of_mut!((*dst).nodes));
            core::ptr::addr_of_mut!((*dst).node_count).write(1);
            core::ptr::addr_of_mut!((*dst).root_id).write(root_id);
            core::ptr::addr_of_mut!((*dst).state).write(GraphState::Active);
            let nodes = &mut *core::ptr::addr_of_mut!((*dst).nodes);
            nodes.write(0, NodeData::new(root_id, root_facet, root_context));
        }
    }

    /// Create a new `LeaseGraph` starting with the root node.
    #[cfg(test)]
    pub(crate) fn new(
        root_id: S::NodeId,
        root_facet: S::Facet,
        root_context: <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
    ) -> Self {
        debug_assert!(S::MAX_NODES > 0, "LeaseGraph requires MAX_NODES > 0");
        debug_assert!(S::MAX_CHILDREN > 0, "LeaseGraph requires MAX_CHILDREN > 0");
        debug_assert!(
            S::MAX_NODES == <S::NodeStorage<'graph> as LeaseNodeStorage<'graph, S>>::CAPACITY,
            "LeaseGraph node storage must match LeaseSpec capacity"
        );
        debug_assert!(
            S::MAX_CHILDREN == <S::ChildStorage as LeaseChildStorage<S::NodeId>>::CAPACITY,
            "LeaseGraph child storage must match LeaseSpec capacity"
        );

        let mut nodes = S::NodeStorage::empty();
        unsafe {
            nodes.write(0, NodeData::new(root_id, root_facet, root_context));
        }

        Self {
            nodes,
            node_count: 1,
            root_id,
            state: GraphState::Active,
        }
    }

    /// Return the root node identifier.
    pub(crate) fn root_id(&self) -> S::NodeId {
        self.root_id
    }

    /// Find the position of a node by identifier.
    fn find_node(&self, id: S::NodeId) -> Option<usize> {
        let mut idx = 0usize;
        while idx < self.node_count {
            let node = self
                .nodes
                .get(idx)
                .expect("active lease graph stores a dense initialized prefix");
            if node.id == id {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    fn find_node_with_taken_mask(&self, id: S::NodeId, taken_mask: usize) -> Option<usize> {
        debug_assert!(
            S::MAX_NODES <= usize::BITS as usize,
            "lease graph traversal mask must cover all node slots"
        );
        let mut idx = 0usize;
        while idx < self.node_count {
            if (taken_mask & (1usize << idx)) != 0 {
                idx += 1;
                continue;
            }
            let node = self
                .nodes
                .get(idx)
                .expect("active lease graph stores a dense initialized prefix");
            if node.id == id {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    /// Obtain a mutable handle for the facet/context associated with `id`.
    ///
    /// ## Typestate
    ///
    /// Only valid while the graph is `Active`; returns `None` afterwards.
    pub(crate) fn handle_mut(&mut self, id: S::NodeId) -> Option<FacetHandle<'_, 'graph, S>> {
        // Reject if the graph already reached a terminal state.
        if self.state != GraphState::Active {
            return None;
        }

        let idx = self.find_node(id)?;
        let node = self.nodes.get_mut(idx)?;
        Some(FacetHandle {
            facet: node.facet,
            context: &mut node.context,
        })
    }

    /// Iterate over the direct children of the specified node.
    #[inline]
    #[cfg(test)]
    pub(crate) fn children(&self, id: S::NodeId) -> Option<ChildIter<'_, 'graph, S>> {
        let idx = self.find_node(id)?;
        let node = self.nodes.get(idx)?;
        Some(ChildIter::new(node))
    }

    /// Obtain a mutable handle for the root facet/context.
    ///
    /// ## Typestate
    ///
    /// Only valid while the graph is `Active`; panics after commit/rollback.
    pub(crate) fn root_handle_mut(&mut self) -> FacetHandle<'_, 'graph, S> {
        self.handle_mut(self.root_id)
            .expect("root node exists and graph is active")
    }

    /// Add a child node under `parent_id` and register `child_facet`.
    ///
    /// ## Typestate
    ///
    /// Only callable while the graph is `Active`; rejected after commit/rollback.
    pub(crate) fn add_child(
        &mut self,
        parent_id: S::NodeId,
        child_id: S::NodeId,
        child_facet: S::Facet,
        child_context: <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>,
    ) -> Result<(), LeaseGraphError> {
        // Reject if the graph already reached a terminal state.
        if self.state == GraphState::Committed {
            return Err(LeaseGraphError::AlreadyCommitted);
        }
        if self.state == GraphState::RolledBack {
            return Err(LeaseGraphError::AlreadyRolledBack);
        }

        // Prevent duplicate node identifiers.
        if self.find_node(child_id).is_some() {
            return Err(LeaseGraphError::DuplicateId);
        }

        // Locate the parent node.
        let parent_idx = self
            .find_node(parent_id)
            .ok_or(LeaseGraphError::NodeNotFound)?;

        // Ensure capacity for another node.
        if self.node_count >= S::MAX_NODES
            || self.node_count >= <S::NodeStorage<'graph> as LeaseNodeStorage<'graph, S>>::CAPACITY
        {
            return Err(LeaseGraphError::GraphFull);
        }

        // Attach the child to its parent.
        {
            self.nodes
                .get_mut(parent_idx)
                .expect("active lease graph stores a dense initialized prefix")
                .add_child(child_id)?;
            unsafe {
                self.nodes
                    .write(self.node_count, NodeData::new(child_id, child_facet, child_context));
            }
        }
        self.node_count += 1;

        Ok(())
    }

    /// Traverse a path of node IDs and return the matching facet.
    ///
    /// `path[0]` must equal `root_id`; every subsequent ID must be a child of
    /// the previous node. Missing links yield `None`.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// // Example path: root → child1 → grandchild
    /// let facet = graph.navigate_mut(&[root_id, child1_id, grandchild_id])?;
    /// ```
    #[cfg(test)]
    pub(crate) fn navigate_mut(
        &mut self,
        path: &[S::NodeId],
    ) -> Option<FacetHandle<'_, 'graph, S>> {
        if path.is_empty() || path[0] != self.root_id {
            return None;
        }

        let mut current = self.root_id;
        for &next_id in &path[1..] {
            let idx = self.find_node(current)?;
            let node = self.nodes.get(idx)?;

            // Ensure the parent lists `next_id` as a child.
            let mut found = false;
            let mut child_idx = 0usize;
            while child_idx < node.child_count {
                if node.children.get(child_idx) == Some(next_id) {
                    found = true;
                    break;
                }
                child_idx += 1;
            }
            if !found {
                return None;
            }
            current = next_id;
        }

        // Return the facet belonging to the final node.
        let idx = self.find_node(current)?;
        let node = self.nodes.get_mut(idx)?;
        Some(FacetHandle {
            facet: node.facet,
            context: &mut node.context,
        })
    }

    /// Consume the graph and commit every node.
    ///
    /// Facets drop in reverse topological order (child → parent).
    pub(crate) fn commit(&mut self) {
        if self.state != GraphState::Active {
            return;
        }
        self.state = GraphState::Committed;
        let mut taken_mask = 0usize;
        self.commit_node_recursive(self.root_id, &mut taken_mask);
        self.node_count = 0;
    }

    fn commit_node_recursive(&mut self, id: S::NodeId, taken_mask: &mut usize) {
        let idx = match self.find_node_with_taken_mask(id, *taken_mask) {
            Some(i) => i,
            None => return,
        };

        let node = self
            .nodes
            .get(idx)
            .expect("active lease graph stores a dense initialized prefix");
        let children = node.children;
        let child_count = node.child_count;

        let mut child_idx = 0usize;
        while child_idx < child_count {
            if let Some(child_id) = children.get(child_idx) {
                self.commit_node_recursive(child_id, taken_mask);
            }
            child_idx += 1;
        }

        *taken_mask |= 1usize << idx;
        let mut node = unsafe { self.nodes.read(idx) };
        node.facet.on_commit(&mut node.context);
    }

    /// Consume the graph and roll back every node.
    ///
    /// Mirrors `commit`: facets drop in reverse topological order.
    pub(crate) fn rollback(&mut self) {
        if self.state != GraphState::Active {
            return;
        }
        self.state = GraphState::RolledBack;
        let mut taken_mask = 0usize;
        self.rollback_node_recursive(self.root_id, &mut taken_mask);
        self.node_count = 0;
    }

    fn rollback_node_recursive(&mut self, id: S::NodeId, taken_mask: &mut usize) {
        let idx = match self.find_node_with_taken_mask(id, *taken_mask) {
            Some(i) => i,
            None => return,
        };

        let node = self
            .nodes
            .get(idx)
            .expect("active lease graph stores a dense initialized prefix");
        let children = node.children;
        let child_count = node.child_count;

        let mut child_idx = 0usize;
        while child_idx < child_count {
            if let Some(child_id) = children.get(child_idx) {
                self.rollback_node_recursive(child_id, taken_mask);
            }
            child_idx += 1;
        }

        *taken_mask |= 1usize << idx;
        let mut node = unsafe { self.nodes.read(idx) };
        node.facet.on_rollback(&mut node.context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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

    #[derive(Clone, Copy, Default)]
    struct TestFacet;

    struct TestContext<'ctx> {
        log: &'ctx RefCell<TestLog>,
        label: &'static str,
        value: u32,
    }

    impl<'ctx> TestContext<'ctx> {
        fn new(log: &'ctx RefCell<TestLog>, label: &'static str, value: u32) -> Self {
            Self { log, label, value }
        }
    }

    impl LeaseFacet for TestFacet {
        type Context<'ctx> = TestContext<'ctx>;

        fn on_commit<'ctx>(&self, context: &mut Self::Context<'ctx>) {
            context.log.borrow_mut().push(context.label);
        }

        fn on_rollback<'ctx>(&self, context: &mut Self::Context<'ctx>) {
            context.log.borrow_mut().push(context.label);
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

    #[test]
    fn handle_updates_context() {
        let log = RefCell::new(TestLog::default());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "root", 1));
        graph
            .add_child(0, 1, TestFacet, TestContext::new(&log, "child", 2))
            .unwrap();

        {
            let mut handle = graph.handle_mut(1).unwrap();
            handle.context().value = 42;
        }

        assert_eq!(graph.handle_mut(1).unwrap().context().value, 42);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn commit_traverses_in_reverse_order() {
        let log = RefCell::new(TestLog::default());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "commit_root", 0));
        graph
            .add_child(0, 1, TestFacet, TestContext::new(&log, "commit_child", 0))
            .unwrap();

        graph.commit();

        assert_eq!(log.borrow().as_slice(), ["commit_child", "commit_root"]);
    }

    #[test]
    fn rollback_traverses_in_reverse_order() {
        let log = RefCell::new(TestLog::default());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "rollback_root", 0));
        graph
            .add_child(0, 1, TestFacet, TestContext::new(&log, "rollback_child", 0))
            .unwrap();

        graph.rollback();

        assert_eq!(log.borrow().as_slice(), ["rollback_child", "rollback_root"]);
    }

    #[test]
    fn navigate_accesses_descendants() {
        let log = RefCell::new(TestLog::default());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "root", 1));
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
        let log = RefCell::new(TestLog::default());
        let mut graph =
            LeaseGraph::<TestSpec>::new(5, TestFacet, TestContext::new(&log, "root", 0));

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
}
