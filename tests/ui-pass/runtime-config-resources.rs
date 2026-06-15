use hibana::runtime::Config;

fn main() {
    let mut slab = [0u8; 64];
    let _ = Config::from_resources(&mut slab[..]);

    let mut slab = [0u8; 64];
    let _ = Config::from_resources(&mut slab);
}
