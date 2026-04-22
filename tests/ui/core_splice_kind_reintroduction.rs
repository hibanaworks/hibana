use hibana::substrate::cap::advanced::{RerouteKind, SpliceAckKind, SpliceIntentKind};

fn main() {
    let _ = (
        core::mem::size_of::<SpliceIntentKind>(),
        core::mem::size_of::<SpliceAckKind>(),
        core::mem::size_of::<RerouteKind>(),
    );
}
