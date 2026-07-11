use hibana::g;

struct NoWireContract;

fn main() {
    let _ = g::send::<0, 1, g::Msg<31, NoWireContract>>();
}
