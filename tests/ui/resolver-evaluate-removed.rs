use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};

static STATE: () = ();

fn choose(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn main() {
    let resolver = ResolverRef::<7>::decision_state(&STATE, choose);
    let _ = resolver.evaluate();
}
