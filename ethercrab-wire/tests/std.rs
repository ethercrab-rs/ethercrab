use ethercrab_wire::EtherCrabWireRead;

#[test]
fn std_string() {
    let input = "Hello world".as_bytes();

    assert_eq!(
        String::unpack_from_slice(input),
        Ok("Hello world".to_string())
    );
}
