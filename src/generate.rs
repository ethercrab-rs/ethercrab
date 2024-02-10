//! Functions used to populate buffers with multiple values.
//!
//! Like cookie_factory but much simpler and will **quite happily panic**.

/// Write a packed struct into the slice.
#[inline(always)]
pub fn write_packed<T>(value: T, buf: &mut [u8]) -> &mut [u8]
where
    T: ethercrab_wire::EtherCrabWireWrite,
{
    value.pack_to_slice_unchecked(buf);

    &mut buf[value.packed_len()..]
}

/// Skip `n` bytes.
pub fn skip(len: usize, buf: &mut [u8]) -> &mut [u8] {
    let len = len.min(buf.len());

    &mut buf[len..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_clamp() {
        let mut buf = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        assert_eq!(skip(10, &mut buf), &[10]);
        assert_eq!(skip(11, &mut buf), &[]);
        assert_eq!(skip(12, &mut buf), &[]);
    }

    #[test]
    fn skip_0() {
        let mut buf = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut expected = buf.clone();

        assert_eq!(skip(0, &mut buf), &mut expected);
    }

    #[test]
    fn skip_1() {
        let mut buf = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut expected = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        assert_eq!(skip(1, &mut buf), &mut expected);
    }

    #[test]
    fn skip_many() {
        let mut buf = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let mut expected = [5, 6, 7, 8, 9, 10];

        assert_eq!(skip(5, &mut buf), &mut expected);
    }
}
