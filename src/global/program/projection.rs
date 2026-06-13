pub(crate) mod seal {
    pub trait Sealed {
        fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE>;
    }
}

/// Bound for hibana choreography values accepted by `project(&value)`.
///
/// This is a sealed projection contract for generated protocol packages,
/// host harnesses, small runtime facades, or other composition layers that
/// need to return `impl Projectable` without naming the internal
/// `hibana::g::Program<_>` step-list type. Ordinary runtime code should
/// build a local `let program = ...` and call the same
/// `runtime::program::project(&program)` entry. Projection authority and
/// metadata visitation stay behind Hibana's projection owner; this trait is
/// only the sealed choreography bound.
///
/// Projection is independent of runtime storage/configuration types. Keep this
/// bound parameter-free so facade APIs do not make users write a runtime
/// parameter just to hide a choreography term.
///
/// The trait is not an extension point. Return hibana choreography values behind
/// `impl Projectable`; do not implement a parallel projection path.
#[diagnostic::on_unimplemented(
    message = "value is not a projectable hibana choreography",
    label = "expected a hibana choreography built with `hibana::g`"
)]
pub trait Projectable: seal::Sealed {}

impl<P> Projectable for P where P: seal::Sealed + ?Sized {}
