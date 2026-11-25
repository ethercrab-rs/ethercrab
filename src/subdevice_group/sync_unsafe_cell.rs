//! A monkey patch over
//! [`core::cell::SyncUnsafeCell`](https://doc.rust-lang.org/stable/core/cell/struct.SyncUnsafeCell.html)
//! until the one from `core` is stabilised.

use core::cell::UnsafeCell;

// MSRV: Remove when core SyncUnsafeCell is stabilised
#[derive(Debug)]
pub(crate) struct MySyncUnsafeCell<T: ?Sized>(pub UnsafeCell<T>);

impl<T> MySyncUnsafeCell<T> {
    pub fn new(inner: T) -> Self {
        Self(UnsafeCell::new(inner))
    }
}

unsafe impl<T: ?Sized + Sync> Sync for MySyncUnsafeCell<T> {}

impl<T: ?Sized> MySyncUnsafeCell<T> {
    /// Gets a mutable pointer to the wrapped value.
    ///
    /// This can be cast to a pointer of any kind.
    /// Ensure that the access is unique (no active references, mutable or not)
    /// when casting to `&mut T`, and ensure that there are no mutations
    /// or mutable aliases going on when casting to `&T`
    #[inline]
    pub const fn get(&self) -> *mut T {
        self.0.get()
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// This call borrows the `SyncUnsafeCell` mutably (at compile-time) which
    /// guarantees that we possess the only reference.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
}
