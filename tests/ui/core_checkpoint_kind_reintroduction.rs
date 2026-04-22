use hibana::substrate::cap::advanced::{CheckpointKind, CommitKind, RollbackKind};

fn main() {
    let _ = (
        core::mem::size_of::<CheckpointKind>(),
        core::mem::size_of::<CommitKind>(),
        core::mem::size_of::<RollbackKind>(),
    );
}
