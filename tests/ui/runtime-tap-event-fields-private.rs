use hibana::runtime::tap::TapEvent;

fn main() {
    let _ = TapEvent { bytes: [0; 16] };
}
