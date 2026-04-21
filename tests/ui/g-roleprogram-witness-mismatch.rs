use hibana::g::advanced::RoleProgram;

fn main() {
    let _ = RoleProgram::<'static, 0, ()> {
        _borrow: core::marker::PhantomData,
        _seal: todo!(),
        summary: todo!(),
        stamp: todo!(),
    };
}
