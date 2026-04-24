use hibana::g;

fn main() {
    let _ = g::send::<g::Role<16>, g::Role<0>, g::Msg<1, ()>, 0>();
}
