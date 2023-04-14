//! A `heapless::Vec`-like storage container but with a smaller API, and the ability to create a
//! reference to it with erased const generics.

use crate::{error::Error, slave::Slave};
use core::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    slice,
};

pub struct SlaveStorage<const N: usize> {
    len: usize,
    slaves: [MaybeUninit<Slave>; N],
}

impl<const N: usize> SlaveStorage<N> {
    const ELEM: MaybeUninit<Slave> = MaybeUninit::uninit();
    const INIT: [MaybeUninit<Slave>; N] = [Self::ELEM; N]; // important for optimization of `new`

    pub const fn new() -> Self {
        Self {
            slaves: Self::INIT,
            len: 0,
        }
    }

    pub fn as_ref(&mut self) -> SlaveStorageRef {
        SlaveStorageRef {
            max_slaves: N,
            len: &mut self.len,
            slaves: &mut self.slaves,
        }
    }
}

impl<const N: usize> Default for SlaveStorage<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Deref for SlaveStorage<N> {
    type Target = [Slave];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.slaves.as_ptr() as *const Slave, self.len) }
    }
}

impl<const N: usize> DerefMut for SlaveStorage<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.slaves.as_ptr() as *mut Slave, self.len) }
    }
}

pub struct SlaveStorageRef<'a> {
    max_slaves: usize,
    len: &'a mut usize,
    slaves: &'a mut [MaybeUninit<Slave>],
}

impl<'a> SlaveStorageRef<'a> {
    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        if *self.len >= self.max_slaves {
            return Err(Error::Capacity(crate::error::Item::Slave));
        }

        unsafe { *self.slaves.get_unchecked_mut(*self.len) = MaybeUninit::new(slave) };

        *self.len += 1;

        Ok(())
    }
}

impl<'a> Deref for SlaveStorageRef<'a> {
    type Target = [Slave];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.slaves.as_ptr() as *const Slave, *self.len) }
    }
}

impl<'a> DerefMut for SlaveStorageRef<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.slaves.as_ptr() as *mut Slave, *self.len) }
    }
}
