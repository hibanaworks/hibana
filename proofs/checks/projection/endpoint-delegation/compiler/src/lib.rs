#![no_std]

//! Proof/export harness for the ownership elaborator used by Hibana's typed
//! source boundary. The source tree and solver have one implementation.

#[cfg(hibana_repo_tests)]
compile_error!("the standalone delegation model must not enable core repository fixtures");

#[path = "../../../../../../src/g/source/tree.rs"]
mod tree;
mod ownership {
    include!("../../../../../../src/g/source/ownership.rs");
    mod observations;
    pub use observations::*;

    #[cfg(test)]
    mod tests;
}
pub use ownership::*;
