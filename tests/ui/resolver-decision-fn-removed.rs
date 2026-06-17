use hibana::runtime::resolver::{ResolverError, ResolverRef};

fn main() {
    let _ = ResolverRef::<7>::decision_fn(|| Err(ResolverError::reject()));
}
