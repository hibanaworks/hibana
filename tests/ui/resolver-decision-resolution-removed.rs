use hibana::runtime::resolver::{DecisionResolution, ResolverError, ResolverRef};

static STATE: () = ();

fn choose(_: &()) -> Result<DecisionResolution, ResolverError> {
    Ok(DecisionResolution::Defer)
}

fn main() {
    let _ = ResolverRef::<7>::decision_state(&STATE, choose);
}
