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
pub(crate) struct InlineLeaseChildStorage<Id: Copy, const CAPACITY: usize> {
    slots: [Option<Id>; CAPACITY],
}

impl<Id: Copy, const CAPACITY: usize> LeaseChildStorage<Id>
    for InlineLeaseChildStorage<Id, CAPACITY>
{
    const CAPACITY: usize = CAPACITY;

    #[inline]
    fn empty() -> Self {
        Self {
            slots: [None; CAPACITY],
        }
    }

    #[inline]
    fn get(&self, idx: usize) -> Option<Id> {
        self.slots.get(idx).copied().flatten()
    }

    #[inline]
    fn set(&mut self, idx: usize, id: Id) {
        self.slots[idx] = Some(id);
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

    fn empty() -> Self;

    fn as_slice(&self) -> &[Option<NodeData<'graph, S>>];

    fn as_mut_slice(&mut self) -> &mut [Option<NodeData<'graph, S>>];
}

pub(crate) struct InlineLeaseNodeStorage<'graph, S: LeaseSpec, const CAPACITY: usize> {
    slots: [Option<NodeData<'graph, S>>; CAPACITY],
}

impl<'graph, S: LeaseSpec, const CAPACITY: usize> LeaseNodeStorage<'graph, S>
    for InlineLeaseNodeStorage<'graph, S, CAPACITY>
{
    const CAPACITY: usize = CAPACITY;

    #[inline]
    fn empty() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
        }
    }

    #[inline]
    fn as_slice(&self) -> &[Option<NodeData<'graph, S>>] {
        &self.slots
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [Option<NodeData<'graph, S>>] {
        &mut self.slots
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
    /// Create a new `LeaseGraph` starting with the root node.
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
        nodes.as_mut_slice()[0] = Some(NodeData::new(root_id, root_facet, root_context));

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
        self.nodes.as_slice()[..self.node_count]
            .iter()
            .position(|node| node.as_ref().map(|n| n.id) == Some(id))
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
        let node = self.nodes.as_mut_slice()[idx].as_mut()?;
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
        let node = self.nodes.as_slice()[idx].as_ref()?;
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
            let nodes = self.nodes.as_mut_slice();
            nodes[parent_idx].as_mut().unwrap().add_child(child_id)?;
            nodes[self.node_count] = Some(NodeData::new(child_id, child_facet, child_context));
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
            let node = self.nodes.as_slice()[idx].as_ref()?;

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
        let node = self.nodes.as_mut_slice()[idx].as_mut()?;
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
        self.commit_node_recursive(self.root_id);
        // All nodes released; reset the counter.
        self.node_count = 0;
    }

    fn commit_node_recursive(&mut self, id: S::NodeId) {
        let idx = match self.find_node(id) {
            Some(i) => i,
            None => return,
        };

        // SAFETY: during commit/rollback `nodes[idx]` is guaranteed to be `Some`.
        let mut node = match self.nodes.as_mut_slice()[idx].take() {
            Some(node) => node,
            None => return,
        };

        // Commit children first.
        let children = node.children;
        let child_count = node.child_count;

        let mut child_idx = 0usize;
        while child_idx < child_count {
            if let Some(child_id) = children.get(child_idx) {
                self.commit_node_recursive(child_id);
            }
            child_idx += 1;
        }

        // Commit the node itself (drop the facet).
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
        self.rollback_node_recursive(self.root_id);
        // All nodes released; reset the counter.
        self.node_count = 0;
    }

    fn rollback_node_recursive(&mut self, id: S::NodeId) {
        let idx = match self.find_node(id) {
            Some(i) => i,
            None => return,
        };

        let mut node = match self.nodes.as_mut_slice()[idx].take() {
            Some(node) => node,
            None => return,
        };

        // Roll back children first.
        let children = node.children;
        let child_count = node.child_count;

        let mut child_idx = 0usize;
        while child_idx < child_count {
            if let Some(child_id) = children.get(child_idx) {
                self.rollback_node_recursive(child_id);
            }
            child_idx += 1;
        }

        // Roll back the node itself (drop the facet).
        node.facet.on_rollback(&mut node.context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::vec::Vec;

    #[derive(Clone, Copy, Default)]
    struct TestFacet;

    struct TestContext<'ctx> {
        log: &'ctx RefCell<Vec<&'static str>>,
        label: &'static str,
        value: u32,
    }

    impl<'ctx> TestContext<'ctx> {
        fn new(log: &'ctx RefCell<Vec<&'static str>>, label: &'static str, value: u32) -> Self {
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
        let log = RefCell::new(Vec::new());
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
        let log = RefCell::new(Vec::new());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "commit_root", 0));
        graph
            .add_child(0, 1, TestFacet, TestContext::new(&log, "commit_child", 0))
            .unwrap();

        graph.commit();

        assert_eq!(log.borrow().as_slice(), &["commit_child", "commit_root"]);
    }

    #[test]
    fn rollback_traverses_in_reverse_order() {
        let log = RefCell::new(Vec::new());
        let mut graph =
            LeaseGraph::<TestSpec>::new(0, TestFacet, TestContext::new(&log, "rollback_root", 0));
        graph
            .add_child(0, 1, TestFacet, TestContext::new(&log, "rollback_child", 0))
            .unwrap();

        graph.rollback();

        assert_eq!(
            log.borrow().as_slice(),
            &["rollback_child", "rollback_root"]
        );
    }

    #[test]
    fn navigate_accesses_descendants() {
        let log = RefCell::new(Vec::new());
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
        let log = RefCell::new(Vec::new());
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
