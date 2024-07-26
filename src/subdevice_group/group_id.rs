/// A group's unique ID.
#[doc(hidden)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupId(pub(in crate::subdevice_group) usize);

impl From<GroupId> for usize {
    fn from(value: GroupId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{subdevice_group::PreOp, SubDeviceGroup};

    #[test]
    fn group_unique_id_defaults() {
        let g1 = SubDeviceGroup::<16, 16, PreOp>::default();
        let g2 = SubDeviceGroup::<16, 16, PreOp>::default();
        let g3 = SubDeviceGroup::<16, 16, PreOp>::default();

        assert_ne!(g1.id, g2.id);
        assert_ne!(g2.id, g3.id);
        assert_ne!(g1.id, g3.id);
    }
}
