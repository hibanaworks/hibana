use hibana::g;

const ROUTE_DECISION: u16 = 7;

fn main() {
    let left = g::send::<0, 1, g::Msg<10, ()>>();
    let right = g::send::<0, 1, g::Msg<20, ()>>();

    let _ = g::route(left, right).roll().resolve::<ROUTE_DECISION>();
}
