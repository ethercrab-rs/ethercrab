#[derive(ethercrab_wire::EtherCrabWireWrite)]
#[repr(usize)]
enum NoUsize {
    Foo,
    Bar,
}

fn main() {}
