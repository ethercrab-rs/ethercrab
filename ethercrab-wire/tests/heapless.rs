use ethercrab_wire::EtherCrabWireRead;

#[test]
fn heapless_str() {
    let input = "Hello world".as_bytes();

    assert_eq!(
        heapless::String::<32>::unpack_from_slice(input),
        Ok("Hello world".try_into().unwrap())
    );
}
