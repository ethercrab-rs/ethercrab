#[cfg(feature = "std")]
#[test]
fn std_string() {
    use ethercrab_wire::EtherCrabWireRead;

    let input = "Hello world".as_bytes();

    assert_eq!(
        String::unpack_from_slice(input),
        Ok("Hello world".to_string())
    );
}
