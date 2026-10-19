use hibana::{Endpoint, g};

async fn moves<'r>(mut owner: Endpoint<'r, 0>, mut db: Endpoint<'r, 10>) {
    let [mut first, _second] = db.par().unwrap();
    owner.send::<g::Msg<1, Endpoint<'r, 10>>>(db).await.unwrap();
    first.send::<g::Msg<2, u32>>(&1).await.unwrap();
}

fn main() {}
