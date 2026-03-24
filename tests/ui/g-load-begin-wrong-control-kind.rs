use hibana::g;
use hibana::g::advanced::CanonicalControl;
use hibana::substrate::cap::{GenericCapToken, advanced::LoadBeginKind};

const LABEL_MGMT_LOAD_BEGIN: u8 = 40;

fn main() {
    let _ = g::send::<
        g::Role<0>,
        g::Role<1>,
        g::Msg<
            { LABEL_MGMT_LOAD_BEGIN },
            GenericCapToken<LoadBeginKind>,
            CanonicalControl<LoadBeginKind>,
        >,
        0,
    >();
}
