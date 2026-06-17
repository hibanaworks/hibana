use hibana::runtime::tap::{Evidence, TapEvent};

fn check(event: TapEvent, evidence: Evidence) {
    let _ = event.causal_role();
    let _ = event.causal_seq();
    let _ = evidence.input_word(0);
}

fn main() {}
