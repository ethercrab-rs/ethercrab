//! A `heapless::Vec`-like storage container but with a smaller API, and the ability to create a
//! reference to it with erased const generics.

use crate::{error::Error, slave::Slave};
use core::{
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

pub struct SlaveStorage<const N: usize> {
    num_slaves: usize,
    slaves: MaybeUninit<[Slave; N]>,
}

impl<const N: usize> SlaveStorage<N> {
    pub const fn new() -> Self {
        Self {
            slaves: MaybeUninit::uninit(),
            num_slaves: 0,
        }
    }

    pub fn as_ref(&mut self) -> SlaveStorageRef {
        SlaveStorageRef {
            max_slaves: N,
            num_slaves: &mut self.num_slaves,
            slaves: unsafe { self.slaves.assume_init_mut() },
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
        let slaves = unsafe { self.slaves.assume_init_ref() };

        &slaves[0..self.num_slaves]
    }
}

impl<const N: usize> DerefMut for SlaveStorage<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let slaves = unsafe { self.slaves.assume_init_mut() };

        &mut slaves[0..self.num_slaves]
    }
}

pub struct SlaveStorageRef<'a> {
    max_slaves: usize,
    num_slaves: &'a mut usize,
    slaves: &'a mut [Slave],
}

impl<'a> SlaveStorageRef<'a> {
    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        if *self.num_slaves >= self.max_slaves {
            return Err(Error::Capacity(crate::error::Item::Slave));
        }

        *self.num_slaves += 1;

        self.slaves[*self.num_slaves] = slave;

        Ok(())
    }
}

impl<'a> Deref for SlaveStorageRef<'a> {
    type Target = [Slave];

    fn deref(&self) -> &Self::Target {
        &self.slaves[0..*self.num_slaves]
    }
}

impl<'a> DerefMut for SlaveStorageRef<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slaves[0..*self.num_slaves]
    }
}
