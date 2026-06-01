use hibana::g;

fn main() {
    let _ = g::send::<16, 0, g::Msg<1, ()>, 0>();
}
