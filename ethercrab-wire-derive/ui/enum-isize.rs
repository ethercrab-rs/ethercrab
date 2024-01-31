#[derive(ethercrab_wire::EtherCrabWireWrite)]
#[repr(isize)]
enum NoIsize {
    Foo,
    Bar,
}

fn main() {}
