use hibana::runtime::resolver::{DecisionResolution, ResolverRef};

fn main() {
    let _ = ResolverRef::<7>::decision_fn(|| Ok(DecisionResolution::Defer));
}
