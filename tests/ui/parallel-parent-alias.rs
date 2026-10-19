use hibana::{Endpoint, g};

fn aliases(endpoint: &mut Endpoint<'_, 0>) {
    let [mut first, _second] = endpoint.par().unwrap();
    let _parent = endpoint.send::<g::Msg<3, u32>>(&3);
    let _child = first.send::<g::Msg<1, u32>>(&1);
}

fn main() {}
