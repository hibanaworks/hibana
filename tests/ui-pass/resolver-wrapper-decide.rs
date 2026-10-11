use hibana::runtime::resolver::{DecisionArm, ResolverError, ResolverRef};

static LOCAL_STATE: () = ();

struct Owner {
    loaded: bool,
    inner: ResolverRef<'static, 7>,
}

fn local(_: &()) -> Result<DecisionArm, ResolverError> {
    Ok(DecisionArm::Left)
}

fn wrapped(owner: &Owner) -> Result<DecisionArm, ResolverError> {
    if owner.loaded {
        Ok(DecisionArm::Right)
    } else {
        owner.inner.decide()
    }
}

fn main() {
    let inner = ResolverRef::<7>::decision_state(&LOCAL_STATE, local);
    let owner = Owner {
        loaded: false,
        inner,
    };
    let resolver = ResolverRef::<7>::decision_state(&owner, wrapped);
    let _ = resolver.decide();
}
