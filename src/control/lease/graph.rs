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
//! use hibana::control::{LeaseGraph, LeaseSpec};
//! use hibana::rendezvous::{SlotBundle, RendezvousId};
//!
//! // Example: Rendezvous IDs and SlotBundle as facets
//! struct RvSlotSpec;
//! impl LeaseSpec for RvSlotSpec {
//!     type NodeId = RendezvousId;
//!     type Facet = SlotBundle<'static, 'static>;
//!     const MAX_NODES: usize = 8;
//!     const MAX_CHILDREN: usize = 4;
//! }
//!
//! // Acquire the root rendezvous slot lease
//! let mut graph = LeaseGraph::<RvSlotSpec>::new(
//!     root_id,
//!     SlotFacet::default(),
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

/// Default upper bound for nodes stored in a LeaseGraph.
const GRAPH_MAX_NODES: usize = 32;

/// Default upper bound for children per node in a LeaseGraph.
const GRAPH_MAX_CHILDREN: usize = 8;

/// LeaseFacet is a zero-sized marker that carries behaviour for commit/rollback
/// while delegating state storage to an explicit context object.
pub trait LeaseFacet: Copy + Default {
    /// Per-node context associated with the facet.
    type Context<'ctx>;

    /// Called during `LeaseGraph::commit` once all children have been committed.
    #[inline]
    fn on_commit<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}

    /// Called during `LeaseGraph::rollback` once all children have been rolled back.
    #[inline]
    fn on_rollback<'ctx>(&self, _context: &mut Self::Context<'ctx>) {}
}

/// LeaseSpec defines the node identifier and facet used by a LeaseGraph.
pub trait LeaseSpec {
    /// Node identifier (e.g. RendezvousId)
    type NodeId: Copy + Eq + Ord + core::fmt::Debug;

    /// Facet marker associated with each node.
    type Facet: LeaseFacet;

    /// Maximum number of nodes (including the root) supported by this graph.
    const MAX_NODES: usize;

    /// Maximum number of children each node may own.
    const MAX_CHILDREN: usize;
}

pub type FacetContext<'graph, S> = <<S as LeaseSpec>::Facet as LeaseFacet>::Context<'graph>;

impl LeaseFacet for () {
    type Context<'ctx> = ();
}

/// Facet used by [`NullLeaseSpec`] to provide a degenerate LeaseGraph configuration.
#[derive(Clone, Copy, Default)]
pub struct NullLeaseFacet;

impl LeaseFacet for NullLeaseFacet {
    type Context<'ctx> = ();
}

/// Lease specification representing the absence of additional lease tracking.
///
/// This is useful for automatons that do not require a [`LeaseGraph`]. When used,
/// the resulting graph contains only the root node paired with the rendezvous ID
/// and an empty context.
#[derive(Clone, Copy, Default)]
pub struct NullLeaseSpec;

impl LeaseSpec for NullLeaseSpec {
    type NodeId = crate::control::types::RendezvousId;
    type Facet = NullLeaseFacet;
    const MAX_NODES: usize = 1;
    const MAX_CHILDREN: usize = 0;
}

/// Global state of the lease graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphState {
    Active,
    Committed,
    RolledBack,
}

/// Internal node payload stored inside the flat array.
struct NodeData<'graph, S: LeaseSpec> {
    id: S::NodeId,
    facet: S::Facet,
    context: FacetContext<'graph, S>,
    /// Array of child node identifiers.
    children: [Option<S::NodeId>; GRAPH_MAX_CHILDREN],
    child_count: usize,
}

impl<'graph, S: LeaseSpec> NodeData<'graph, S> {
    fn new(id: S::NodeId, facet: S::Facet, context: FacetContext<'graph, S>) -> Self {
        Self {
            id,
            facet,
            context,
            children: [None; GRAPH_MAX_CHILDREN],
            child_count: 0,
        }
    }

    fn add_child(&mut self, child_id: S::NodeId) -> Result<(), LeaseGraphError> {
        if self.child_count >= S::MAX_CHILDREN || self.child_count >= GRAPH_MAX_CHILDREN {
            return Err(LeaseGraphError::TooManyChildren);
        }
        self.children[self.child_count] = Some(child_id);
        self.child_count += 1;
        Ok(())
    }

    fn remove_child(&mut self, child_id: S::NodeId) -> bool {
        for i in 0..self.child_count {
            if self.children[i] == Some(child_id) {
                // Move the last element into the slot we are removing.
                if i < self.child_count - 1 {
                    self.children[i] = self.children[self.child_count - 1];
                }
                self.children[self.child_count - 1] = None;
                self.child_count -= 1;
                return true;
            }
        }
        false
    }
}

/// Iterator over a node's direct children.
pub struct ChildIter<'a, 'graph, S: LeaseSpec> {
    node: &'a NodeData<'graph, S>,
    index: usize,
}

impl<'a, 'graph, S: LeaseSpec> ChildIter<'a, 'graph, S> {
    fn new(node: &'a NodeData<'graph, S>) -> Self {
        Self { node, index: 0 }
    }
}

impl<'a, 'graph, S: LeaseSpec> Iterator for ChildIter<'a, 'graph, S> {
    type Item = S::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.node.child_count {
            let slot = self.index;
            self.index += 1;
            if let Some(id) = self.node.children[slot] {
                return Some(id);
            }
        }
        None
    }
}

/// Mutable handle exposing a facet marker and its associated context.
pub struct FacetHandle<'a, 'graph, S: LeaseSpec> {
    facet: S::Facet,
    context: &'a mut FacetContext<'graph, S>,
}

impl<'a, 'graph, S: LeaseSpec> FacetHandle<'a, 'graph, S> {
    #[inline]
    pub fn facet(&self) -> S::Facet {
        self.facet
    }

    #[inline]
    pub fn context(&mut self) -> &mut FacetContext<'graph, S> {
        self.context
    }

    #[inline]
    pub fn with<R>(self, f: impl FnOnce(S::Facet, &mut FacetContext<'graph, S>) -> R) -> R {
        let FacetHandle { facet, context } = self;
        f(facet, context)
    }
}

/// Errors emitted by `LeaseGraph` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseGraphError {
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
pub struct LeaseGraph<'graph, S: LeaseSpec> {
    /// Storage backing every node.
    nodes: [Option<NodeData<'graph, S>>; GRAPH_MAX_NODES],
    /// Number of nodes currently present.
    node_count: usize,
    /// Identifier of the root node.
    root_id: S::NodeId,
    /// Current state of the graph.
    state: GraphState,
}

impl<'graph, S: LeaseSpec> LeaseGraph<'graph, S> {
    /// Create a new `LeaseGraph` starting with the root node.
    pub fn new(
        root_id: S::NodeId,
        root_facet: S::Facet,
        root_context: FacetContext<'graph, S>,
    ) -> Self {
        debug_assert!(S::MAX_NODES > 0, "LeaseGraph requires MAX_NODES > 0");
        debug_assert!(S::MAX_CHILDREN > 0, "LeaseGraph requires MAX_CHILDREN > 0");
        debug_assert!(
            S::MAX_NODES <= GRAPH_MAX_NODES,
            "LeaseGraph MAX_NODES exceeds storage capacity"
        );
        debug_assert!(
            S::MAX_CHILDREN <= GRAPH_MAX_CHILDREN,
            "LeaseGraph MAX_CHILDREN exceeds storage capacity"
        );

        let mut nodes: [Option<NodeData<'graph, S>>; GRAPH_MAX_NODES] = Default::default();
        nodes[0] = Some(NodeData::new(root_id, root_facet, root_context));

        Self {
            nodes,
            node_count: 1,
            root_id,
            state: GraphState::Active,
        }
    }

    /// Return the root node identifier.
    pub fn root_id(&self) -> S::NodeId {
        self.root_id
    }

    /// Find the position of a node by identifier.
    fn find_node(&self, id: S::NodeId) -> Option<usize> {
        self.nodes[..self.node_count]
            .iter()
            .position(|node| node.as_ref().map(|n| n.id) == Some(id))
    }

    /// Obtain a mutable handle for the facet/context associated with `id`.
    ///
    /// ## Typestate
    ///
    /// Only valid while the graph is `Active`; returns `None` afterwards.
    pub fn handle_mut(&mut self, id: S::NodeId) -> Option<FacetHandle<'_, 'graph, S>> {
        // Reject if the graph already reached a terminal state.
        if self.state != GraphState::Active {
            return None;
        }

        let idx = self.find_node(id)?;
        let node = self.nodes[idx].as_mut()?;
        Some(FacetHandle {
            facet: node.facet,
            context: &mut node.context,
        })
    }

    #[inline]
    pub fn facet_mut(&mut self, id: S::NodeId) -> Option<FacetHandle<'_, 'graph, S>> {
        self.handle_mut(id)
    }

    /// Iterate over the direct children of the specified node.
    #[inline]
    pub fn children(&self, id: S::NodeId) -> Option<ChildIter<'_, 'graph, S>> {
        let idx = self.find_node(id)?;
        let node = self.nodes[idx].as_ref()?;
        Some(ChildIter::new(node))
    }

    /// Obtain a mutable handle for the root facet/context.
    ///
    /// ## Typestate
    ///
    /// Only valid while the graph is `Active`; panics after commit/rollback.
    pub fn root_handle_mut(&mut self) -> FacetHandle<'_, 'graph, S> {
        self.handle_mut(self.root_id)
            .expect("root node exists and graph is active")
    }

    #[inline]
    pub fn root_facet_mut(&mut self) -> FacetHandle<'_, 'graph, S> {
        self.root_handle_mut()
    }

    /// Add a child node under `parent_id` and register `child_facet`.
    ///
    /// ## Typestate
    ///
    /// Only callable while the graph is `Active`; rejected after commit/rollback.
    pub fn add_child(
        &mut self,
        parent_id: S::NodeId,
        child_id: S::NodeId,
        child_facet: S::Facet,
        child_context: FacetContext<'graph, S>,
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
        if self.node_count >= S::MAX_NODES || self.node_count >= GRAPH_MAX_NODES {
            return Err(LeaseGraphError::GraphFull);
        }

        // Attach the child to its parent.
        self.nodes[parent_idx]
            .as_mut()
            .unwrap()
            .add_child(child_id)?;

        // Persist the new child node in storage.
        self.nodes[self.node_count] = Some(NodeData::new(child_id, child_facet, child_context));
        self.node_count += 1;

        Ok(())
    }

    /// Remove a child node (detaching it from the parent).
    ///
    /// All descendants of the removed node are pruned recursively.
    ///
    /// ## Typestate
    ///
    /// Only callable while the graph is `Active`; rejected after commit/rollback.
    pub fn remove_child(
        &mut self,
        parent_id: S::NodeId,
        child_id: S::NodeId,
    ) -> Result<(), LeaseGraphError> {
        // Reject if the graph already reached a terminal state.
        if self.state == GraphState::Committed {
            return Err(LeaseGraphError::AlreadyCommitted);
        }
        if self.state == GraphState::RolledBack {
            return Err(LeaseGraphError::AlreadyRolledBack);
        }

        // Locate the parent node.
        let parent_idx = self
            .find_node(parent_id)
            .ok_or(LeaseGraphError::NodeNotFound)?;

        // Detach the child from its parent.
        if !self.nodes[parent_idx]
            .as_mut()
            .unwrap()
            .remove_child(child_id)
        {
            return Err(LeaseGraphError::NodeNotFound);
        }

        // Remove the child and all descendants.
        self.remove_node_recursive(child_id);

        Ok(())
    }

    /// Recursively prune a node and all descendants.
    fn remove_node_recursive(&mut self, id: S::NodeId) {
        let idx = match self.find_node(id) {
            Some(i) => i,
            None => return,
        };

        // Remove descendants first.
        let children: [Option<S::NodeId>; GRAPH_MAX_CHILDREN] =
            self.nodes[idx].as_ref().unwrap().children;
        let child_count = self.nodes[idx].as_ref().unwrap().child_count;

        for child_id in children[..child_count].iter().filter_map(|id| *id) {
            self.remove_node_recursive(child_id);
        }

        // Remove the node itself.
        self.nodes[idx] = None;

        // Compact the array by moving the last occupied slot.
        if idx < self.node_count - 1 {
            self.nodes[idx] = self.nodes[self.node_count - 1].take();
        }
        self.node_count -= 1;
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
    pub fn navigate_mut(&mut self, path: &[S::NodeId]) -> Option<FacetHandle<'_, 'graph, S>> {
        if path.is_empty() || path[0] != self.root_id {
            return None;
        }

        let mut current = self.root_id;
        for &next_id in &path[1..] {
            let idx = self.find_node(current)?;
            let node = self.nodes[idx].as_ref()?;

            // Ensure the parent lists `next_id` as a child.
            if !node.children[..node.child_count].contains(&Some(next_id)) {
                return None;
            }
            current = next_id;
        }

        // Return the facet belonging to the final node.
        let idx = self.find_node(current)?;
        let node = self.nodes[idx].as_mut()?;
        Some(FacetHandle {
            facet: node.facet,
            context: &mut node.context,
        })
    }

    /// Consume the graph and commit every node.
    ///
    /// Facets drop in reverse topological order (child → parent).
    pub fn commit(&mut self) {
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
        let mut node = match self.nodes[idx].take() {
            Some(node) => node,
            None => return,
        };

        // Commit children first.
        let children = node.children;
        let child_count = node.child_count;

        for child_id in children[..child_count].iter().filter_map(|id| *id) {
            self.commit_node_recursive(child_id);
        }

        // Commit the node itself (drop the facet).
        node.facet.on_commit(&mut node.context);
    }

    /// Consume the graph and roll back every node.
    ///
    /// Mirrors `commit`: facets drop in reverse topological order.
    pub fn rollback(&mut self) {
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

        let mut node = match self.nodes[idx].take() {
            Some(node) => node,
            None => return,
        };

        // Roll back children first.
        let children = node.children;
        let child_count = node.child_count;

        for child_id in children[..child_count].iter().filter_map(|id| *id) {
            self.rollback_node_recursive(child_id);
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
