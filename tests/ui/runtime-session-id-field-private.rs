use hibana::runtime::ids::SessionId;

fn main() {
    let _ = SessionId(1);
    let _ = SessionId::new(1);
}
