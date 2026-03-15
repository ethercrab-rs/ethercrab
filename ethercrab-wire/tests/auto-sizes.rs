use ethercrab_wire::EtherCrabWireReadWrite;

#[test]
fn auto_sizes() {
    #[derive(Debug, EtherCrabWireReadWrite, PartialEq)]
    #[wire(bytes = 46)]
    struct Check {
        a: u8,
        b: i8,
        c: u16,
        d: i16,
        e: u32,
        f: i32,
        g: u64,
        h: i64,
        i: f32,
        j: f64,
    }
}
