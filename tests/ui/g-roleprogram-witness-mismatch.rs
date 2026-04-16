use hibana::g::advanced::RoleProgram;
use hibana::substrate::cap::advanced::{MintConfig, MintConfigMarker};

fn main() {
    let _ = RoleProgram::<'static, 0, MintConfig> {
        _borrow: core::marker::PhantomData,
        _seal: todo!(),
        summary: todo!(),
        mint: MintConfig::INSTANCE,
        stamp: todo!(),
    };
}
