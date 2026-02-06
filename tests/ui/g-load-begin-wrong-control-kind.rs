use hibana::control::cap::{
    resource_kinds::LoadBeginKind,
    GenericCapToken,
};
use hibana::g;
use hibana::runtime::consts::LABEL_MGMT_LOAD_BEGIN;

type BadMsg = g::Msg<
    { LABEL_MGMT_LOAD_BEGIN },
    GenericCapToken<LoadBeginKind>,
    g::CanonicalControl<LoadBeginKind>,
>;

fn main() {
    let _ = g::send::<g::Role<0>, g::Role<1>, BadMsg>();
}
