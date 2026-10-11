use hibana::g;

fn main() {
    let _ = g::send::<0, 0, g::Msg<1, u8>>();
}
