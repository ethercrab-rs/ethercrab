use ethercrab_derive::EtherCrabWire;

#[derive(EtherCrabWire)]
union Whatever {
    i: u32,
    f: f32,
}

fn main() {}
