use hibana::substrate::cap::advanced::{
    LoadBeginKind, LoadCommitKind, PolicyActivateKind, PolicyAnnotateKind, PolicyLoadKind,
    PolicyRevertKind,
};

fn main() {
    let _ = (
        core::mem::size_of::<PolicyLoadKind>(),
        core::mem::size_of::<PolicyActivateKind>(),
        core::mem::size_of::<PolicyRevertKind>(),
        core::mem::size_of::<PolicyAnnotateKind>(),
        core::mem::size_of::<LoadBeginKind>(),
        core::mem::size_of::<LoadCommitKind>(),
    );
}
