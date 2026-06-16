use hibana::runtime::resolver::{DecisionResolution, ResolverError, ResolverRef};

static UNIT: () = ();

fn choose(_: &()) -> Result<DecisionResolution, ResolverError> {
    Ok(DecisionResolution::Defer)
}

fn main() {
    let resolver = ResolverRef::<9>::decision_state(&UNIT, choose);
    let _ = resolver.evaluate();
}
