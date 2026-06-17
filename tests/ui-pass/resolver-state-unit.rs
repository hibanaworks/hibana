use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};

static UNIT: () = ();

fn choose(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn main() {
    let resolver = ResolverRef::<9>::decision_state(&UNIT, choose);
    let _ = resolver.evaluate();
}
