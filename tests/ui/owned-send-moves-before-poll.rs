use hibana::{Endpoint, g};

type Loan = g::Msg<1, Endpoint<'static, 10>>;
type Read = g::Msg<2, u32>;

fn reuse_after_move<'r>(owner: &mut Endpoint<'r, 0>, mut db: Endpoint<'r, 10>) {
    let pending = owner.send::<Loan>(db);
    let _operation = db.recv::<Read>();
    drop(pending);
}

fn main() {}
