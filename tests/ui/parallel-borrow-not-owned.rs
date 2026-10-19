use hibana::{Endpoint, g};

async fn sends_borrow<'r>(mut owner: Endpoint<'r, 0>, mut db: Endpoint<'r, 10>) {
    let [first, _second] = db.par().unwrap();
    owner
        .send::<g::Msg<1, Endpoint<'r, 10>>>(first)
        .await
        .unwrap();
}

fn main() {}
