use hibana::substrate::program::RoleProgram;

fn main() {
    let _ = RoleProgram::<'static, 0, ()> {
        _borrow: core::marker::PhantomData,
        _seal: todo!(),
        summary: todo!(),
        stamp: todo!(),
    };
}
