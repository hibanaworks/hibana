use hibana::integration::cap::control::{CheckpointKind, CommitKind, RollbackKind};

fn main() {
    let _ = (
        core::mem::size_of::<CheckpointKind>(),
        core::mem::size_of::<CommitKind>(),
        core::mem::size_of::<RollbackKind>(),
    );
}
