use ethercrab_derive::EtherCatWire;

#[derive(EtherCatWire)]
union Whatever {
    i: u32,
    f: f32,
}

fn main() {}
