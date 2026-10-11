fn main() {
    let mut slab = [0u8; 64];
    let _ = hibana::runtime::Config::from_resources(&mut slab);
}
