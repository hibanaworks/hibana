/// Neutral program-level facts emitted by projection metadata visitors.
///
/// These facts describe the projected hibana program shape only. They do not
/// name WASI, boards, sites, or any downstream runtime concept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionProgramFacts {
    pub role_count: u8,
    pub eff_count: u16,
    pub scope_count: u16,
    pub route_scope_count: u16,
    pub parallel_enter_count: u16,
    pub control_scope_mask: u8,
    pub fingerprint: [u64; 2],
}

/// Neutral atom facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionAtomSpec {
    pub eff_index: u16,
    pub from: u8,
    pub to: u8,
    pub label: u8,
    pub lane: u8,
    pub is_control: bool,
    pub resource: Option<u8>,
    pub control_scope: Option<u8>,
    pub control_path: Option<u8>,
    pub control_shot: Option<u8>,
    pub control_op: Option<u8>,
    pub control_tap_id: Option<u16>,
}

/// Neutral policy facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionPolicySpec {
    pub eff_index: u16,
    pub policy_id: u16,
    pub scope_raw: u64,
}

/// Neutral scope facts emitted by projection metadata visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionScopeSpec {
    pub offset: u16,
    pub scope_raw: u64,
    pub scope_kind: u8,
    pub event: u8,
    pub linger: bool,
    pub controller_role: Option<u8>,
}

/// Visitor for neutral projection metadata.
///
/// Downstream crates should derive capacity from these official projection
/// facts instead of deriving meaning from helper names, labels as strings, or
/// an appkit-specific choreography wrapper.
pub trait ProjectionMetadataVisitor {
    fn visit_program(&mut self, _: ProjectionProgramFacts) {}

    fn visit_atom(&mut self, _: ProjectionAtomSpec) {}

    fn visit_policy(&mut self, _: ProjectionPolicySpec) {}

    fn visit_scope(&mut self, _: ProjectionScopeSpec) {}
}

/// Public substrate bound for unnamed hibana choreography values.
///
/// This is a substrate contract for wrappers such as pico appkits, host
/// harnesses, generated protocol packages, or other integration layers that
/// need to return or store `impl Projectable<Universe>` without naming the
/// internal `hibana::g::Program<_>` step-list type. Ordinary integration code
/// should build a local `let program = ...` and call
/// `integration::program::project(&program)`.
///
/// Downstream implementations are advanced integration points. They must expose
/// the same role projection and metadata facts as the underlying hibana
/// choreography value; they are not needed for ordinary application code.
#[diagnostic::on_unimplemented(
    message = "value is not a projectable hibana choreography",
    label = "expected a hibana choreography built with `hibana::g`"
)]
pub trait Projectable<Universe> {
    fn visit_projection_metadata<V: ProjectionMetadataVisitor>(&self, visitor: &mut V);

    /// Generic substrate projection hook.
    ///
    /// Prefer the free `project(&program)` function in ordinary integration
    /// code.
    fn project<const ROLE: u8>(&self) -> crate::global::role_program::RoleProgram<ROLE>;
}
