use bbqueue::BBBuffer;

fn main() {
    // Create a buffer with six elements
    let bb: BBBuffer<6> = BBBuffer::new();
    let (mut prod, mut cons) = bb.try_split().unwrap();

    let mut wgr = prod.grant_exact(1).expect("Write 1");
    wgr[0] = 123;
    wgr.commit(1);

    let mut wgr = prod.grant_exact(1).expect("Write 2");
    wgr[0] = 231;
    wgr.commit(1);

    let rgr = cons.read().expect("Read 1");
    // Panics with grant in progress error
    let rgr2 = cons.read().expect("Read 2");

    assert_eq!(rgr[0], 123);
    rgr.release(1);

    assert_eq!(rgr2[0], 231);
    rgr2.release(1);
}
