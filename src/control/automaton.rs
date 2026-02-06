//! Control automaton hub.

/// Delegation automata.
pub mod delegation;
/// Distributed splice automata.
pub mod distributed;
/// Splice automata.
pub mod splice;
/// Transaction automata.
pub mod txn;

pub use delegation::*;
pub use distributed::*;
pub use splice::*;
pub use txn::*;
