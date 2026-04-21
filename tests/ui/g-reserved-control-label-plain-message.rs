use hibana::g;

fn main() {
    let _ = g::send::<g::Role<0>, g::Role<1>, g::Msg<110, u32>, 0>();
}
