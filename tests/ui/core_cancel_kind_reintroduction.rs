use hibana::integration::cap::control::{CancelAckKind, CancelKind};

fn main() {
    let _ = (
        core::mem::size_of::<CancelKind>(),
        core::mem::size_of::<CancelAckKind>(),
    );
}
