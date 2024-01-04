use ethercrab_derive::EtherCrabWireWrite;

#[derive(EtherCrabWireWrite)]
union Whatever {
    i: u32,
    f: f32,
}

fn main() {}
