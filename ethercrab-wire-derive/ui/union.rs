use ethercrab_derive::EtherCrabWireReadWrite;

#[derive(EtherCrabWireReadWrite)]
union Whatever {
    i: u32,
    f: f32,
}

fn main() {}
