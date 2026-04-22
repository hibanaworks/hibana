use hibana::substrate::cap::advanced::{CancelAckKind, CancelKind};

fn main() {
    let _ = (
        core::mem::size_of::<CancelKind>(),
        core::mem::size_of::<CancelAckKind>(),
    );
}
