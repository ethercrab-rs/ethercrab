/// A group's unique ID.
#[doc(hidden)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupId(pub(in crate::slave_group) usize);

impl From<GroupId> for usize {
    fn from(value: GroupId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{slave_group::PreOp, SlaveGroup};

    #[test]
    fn group_unique_id_defaults() {
        let g1 = SlaveGroup::<16, 16, PreOp>::default();
        let g2 = SlaveGroup::<16, 16, PreOp>::default();
        let g3 = SlaveGroup::<16, 16, PreOp>::default();

        assert_ne!(g1.id, g2.id);
        assert_ne!(g2.id, g3.id);
        assert_ne!(g1.id, g3.id);
    }

    #[test]
    fn group_unique_id_same_fn() {
        let g1 = SlaveGroup::<16, 16, PreOp>::new();
        let g2 = SlaveGroup::<16, 16, PreOp>::new();

        assert_ne!(g1.id, g2.id);
    }
}
