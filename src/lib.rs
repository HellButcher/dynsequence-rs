#![warn(
    // missing_docs,
    // rustdoc::missing_doc_code_examples,
    future_incompatible,
    rust_2018_idioms,
    unused,
    trivial_casts,
    trivial_numeric_casts,
    unused_lifetimes,
    unused_qualifications,
    unused_crate_dependencies,
    clippy::cargo,
    clippy::multiple_crate_versions,
    clippy::empty_line_after_outer_attr,
    clippy::fallible_impl_from,
    clippy::redundant_pub_crate,
    clippy::use_self,
    clippy::suspicious_operation_groupings,
    clippy::useless_let_if_seq,
    // clippy::missing_errors_doc,
    // clippy::missing_panics_doc,
    clippy::wildcard_imports
)]
#![doc(html_no_source)]
#![no_std]
#![doc = include_str!("../README.md")]
#![cfg_attr(feature = "unstable", feature(unsize))]

extern crate alloc;

use alloc::vec::Vec;
use core::{
    iter::FusedIterator,
    mem,
    ops::{Deref, DerefMut, Index, IndexMut},
    ptr,
};

#[cfg(feature = "unstable")]
use core::marker::Unsize;

pub struct DynBlocks {
    block_size: usize,
    max_block_size: usize,
    blocks: Vec<(*mut u8, usize)>,
    next_block_offset: usize,
}

impl DynBlocks {
    pub const fn new() -> Self {
        Self::with_blocksize(128)
    }

    pub const fn with_blocksize(block_size: usize) -> Self {
        const MAX_BLOCK_SIZE: usize = 2048;
        Self {
            block_size,
            max_block_size: if block_size < MAX_BLOCK_SIZE {
                MAX_BLOCK_SIZE
            } else {
                block_size
            },
            blocks: Vec::new(),
            next_block_offset: 0,
        }
    }

    fn next_ptr(&mut self, size: usize, align: usize) -> *mut u8 {
        if let Some((ptr, len)) = self.blocks.last_mut().copied() {
            let ptr = unsafe { ptr.add(self.next_block_offset) };
            let align_offset = ptr.align_offset(align);
            if self.next_block_offset + align_offset + size <= len {
                self.next_block_offset += align_offset + size;
                return ptr;
            }
        }
        // allocate a new block
        if size > self.max_block_size {
            // use exact size block
            unsafe {
                let layout = alloc::alloc::Layout::from_size_align_unchecked(size, align);
                let ptr_exact = alloc::alloc::alloc_zeroed(layout);
                self.next_block_offset = size;
                self.blocks.push((ptr_exact, size));
                return ptr_exact;
            }
        }
        // use power-of-2 blocksizes (increment)
        let block_size = size.max(self.block_size).next_power_of_two();
        if block_size * 2 <= self.max_block_size {
            self.block_size = block_size * 2;
        }
        unsafe {
            let layout = alloc::alloc::Layout::from_size_align_unchecked(block_size, align);
            let ptr = alloc::alloc::alloc_zeroed(layout);
            self.next_block_offset = size;
            self.blocks.push((ptr, block_size));
            ptr
        }
    }

    /// # Safety
    ///
    /// Behavior is undefined if any of the following conditions are violated:
    ///
    /// * `src` must be valid for reads.
    /// * `src` must be properly aligned.
    /// * `src` must point to a properly initialized value of type `T`.
    /// * `src` must not be used or dropped anymore (use std::mem::forget)
    pub unsafe fn push_raw<T: ?Sized>(&mut self, src: *const T) -> DynBlockRef<'_, T> {
        let size = mem::size_of_val::<T>(&*src);
        let align = mem::align_of_val::<T>(&*src);
        let dst = self.next_ptr(size, align);
        ptr::copy_nonoverlapping(src as *const u8, dst, size);

        let mut r = src as *mut T;
        ptr::write(ptr::addr_of_mut!(r), src as *mut T);
        ptr::write(ptr::addr_of_mut!(r) as *mut *mut u8, dst); // replace address

        DynBlockRef(&mut *r)
    }

    #[inline]
    pub fn push<T>(&mut self, value: T) -> DynBlockRef<'_, T> {
        unsafe {
            let r = self.push_raw(&value);
            mem::forget(value);
            r
        }
    }
}

pub struct DynBlockRef<'l, T: ?Sized>(&'l mut T);

impl<'l, T: ?Sized> DynBlockRef<'l, T> {
    pub fn into_ref(self) -> &'l T {
        let r: *const T = self.0;
        unsafe {
            mem::forget(self);
            &*r
        }
    }
    pub fn into_mut(self) -> &'l mut T {
        let r: *mut T = self.0;
        unsafe {
            mem::forget(self);
            &mut *r
        }
    }

    pub fn as_ptr(&mut self) -> *const T {
        self.0
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.0
    }
}

impl<'l, T: ?Sized> Deref for DynBlockRef<'l, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'l, T: ?Sized> DerefMut for DynBlockRef<'l, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl<'l, T: ?Sized> Drop for DynBlockRef<'l, T> {
    fn drop(&mut self) {
        unsafe { ptr::drop_in_place(self.0) }
    }
}

pub struct DynSequence<T: ?Sized> {
    ptrs: Vec<*mut T>,
    blocks: DynBlocks,
}

impl<T: ?Sized> DynSequence<T> {
    pub const fn new() -> Self {
        Self::with_blocksize(128)
    }

    pub const fn with_blocksize(block_size: usize) -> Self {
        Self {
            ptrs: Vec::new(),
            blocks: DynBlocks::with_blocksize(block_size),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ptrs.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ptrs.is_empty()
    }

    /// Inserts a new value into the DynSequence at the given index (see Vec::insert)
    /// by moving the value behind the pointer and taking ownership.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if any of the following conditions are violated:
    ///
    /// * `src` must be valid for reads.
    /// * `src` must be properly aligned.
    /// * `src` must point to a properly initialized value of type `T`.
    /// * `src` must not be used or dropped anymore (use std::mem::forget)
    pub unsafe fn insert_raw(&mut self, index: usize, src: *const T) -> &mut T {
        let mut r = self.blocks.push_raw(src);
        self.ptrs.insert(index, r.as_mut_ptr());
        r.into_mut()
    }

    /// Adds a new value at the end of the DynSequence (see Vec::push)
    /// by moving the value behind the pointer and taking ownership.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if any of the following conditions are violated:
    ///
    /// * `src` must be valid for reads.
    /// * `src` must be properly aligned.
    /// * `src` must point to a properly initialized value of type `T`.
    /// * `src` must not be used or dropped anymore (use std::mem::forget)
    pub unsafe fn push_raw(&mut self, src: *const T) -> &mut T {
        let mut r = self.blocks.push_raw(src);
        self.ptrs.push(r.as_mut_ptr());
        r.into_mut()
    }

    #[cfg(feature = "unstable")]
    #[inline]
    pub fn insert<U>(&mut self, index: usize, src: U) -> &mut T
    where
        U: Unsize<T>,
    {
        unsafe {
            let r = self.insert_raw(index, &src);
            mem::forget(src);
            r
        }
    }

    #[cfg(feature = "unstable")]
    #[inline]
    pub fn push<U>(&mut self, src: U) -> &mut T
    where
        U: Unsize<T>,
    {
        unsafe {
            let r = self.push_raw(&src);
            mem::forget(src);
            r
        }
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        let p: *mut T = *self.ptrs.get(index)?;
        unsafe { Some(&*p) }
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let p: *mut T = *self.ptrs.get_mut(index)?;
        unsafe { Some(&mut *p) }
    }

    #[inline]
    pub fn as_slice(&self) -> &[&T] {
        unsafe { mem::transmute(self.ptrs.as_slice()) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [&mut T] {
        unsafe { mem::transmute(self.ptrs.as_mut_slice()) }
    }

    #[inline]
    pub fn iter(&self) -> DynSeqIter<'_, T> {
        self.as_slice().iter().copied()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> DynSeqIterMut<'_, T> {
        DynSeqIterMut(self.as_mut_slice().iter_mut())
    }
}

impl<T: ?Sized> Index<usize> for DynSequence<T> {
    type Output = T;
    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("index out of bounds")
    }
}

impl<T: ?Sized> IndexMut<usize> for DynSequence<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.get_mut(index).expect("index out of bounds")
    }
}

pub type DynSeqIter<'l, T> = core::iter::Copied<core::slice::Iter<'l, &'l T>>;
pub struct DynSeqIterMut<'l, T: ?Sized>(core::slice::IterMut<'l, &'l mut T>);

impl<'l, T: ?Sized> Iterator for DynSeqIterMut<'l, T> {
    type Item = &'l mut T;
    #[inline]
    fn next(&mut self) -> Option<&'l mut T> {
        Some(self.0.next()?)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }

    fn nth(&mut self, n: usize) -> Option<&'l mut T> {
        Some(self.0.nth(n)?)
    }

    fn last(self) -> Option<&'l mut T> {
        Some(self.0.last()?)
    }

    fn count(self) -> usize {
        self.0.count()
    }
}

impl<'l, T: ?Sized> DoubleEndedIterator for DynSeqIterMut<'l, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'l mut T> {
        Some(self.0.next_back()?)
    }
}

impl<'l, T: ?Sized> ExactSizeIterator for DynSeqIterMut<'l, T> {
    #[inline]
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'l, T: ?Sized> FusedIterator for DynSeqIterMut<'l, T> {}

impl<'l, T: ?Sized> IntoIterator for &'l DynSequence<T> {
    type Item = &'l T;
    type IntoIter = DynSeqIter<'l, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'l, T: ?Sized> IntoIterator for &'l mut DynSequence<T> {
    type Item = &'l mut T;
    type IntoIter = DynSeqIterMut<'l, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T: ?Sized> Drop for DynSequence<T> {
    fn drop(&mut self) {
        for p in self.ptrs.drain(..) {
            unsafe {
                ptr::drop_in_place(p);
            }
        }
    }
}

#[doc(hidden)]
pub use ::core::mem::forget as __forget;
#[doc(hidden)]
pub mod __macro_raw_fn {
    #[inline(always)]
    pub unsafe fn push<T: ?Sized>(dst: &mut crate::DynSequence<T>, raw: *mut T) {
        dst.push_raw(raw);
    }
    #[inline(always)]
    pub unsafe fn insert<T: ?Sized>(dst: &mut crate::DynSequence<T>, index: usize, raw: *mut T) {
        dst.insert_raw(index, raw);
    }
}

#[macro_export]
macro_rules! dyn_sequence {
    [$t:ty | $r:expr => { $(
        $m:ident ($e:expr ) $(@ $i:expr)?;
    )* } ] => {
        {
            let r: &mut $crate::DynSequence<$t> = $r;
            $(
                #[allow(unsafe_code, clippy::forget_copy, clippy::forget_ref)]
                unsafe {
                    let mut v = $e;
                    $crate::__macro_raw_fn::$m(r, $($i,)? &mut v);
                    $crate::__forget(v);
                }
            )*
        }
    };
    [$t:ty => $($e:expr),*] => {
        {
            let mut r: $crate::DynSequence::<$t> = $crate::DynSequence::new();
            $(
                #[allow(unsafe_code, clippy::forget_copy, clippy::forget_ref)]
                unsafe {
                    let mut v = $e;
                    $crate::__macro_raw_fn::push(&mut r, &mut v);
                    $crate::__forget(v);
                }
            )*
            r
        }
    };
}

#[cfg(test)]
mod tests {
    use core::any::Any;

    use crate::DynSequence;

    #[test]
    fn test_ctor_macro() {
        let seq: DynSequence<dyn Any> = dyn_sequence![dyn Any =>
            123u8,
            true,
            456u16,
            "Hallo Welt!"
        ];

        assert_eq!(
            Some(&123u8),
            seq.get(0).and_then(|a| a.downcast_ref::<u8>())
        );
        assert_eq!(None, seq.get(0).and_then(|a| a.downcast_ref::<u16>()));
        assert_eq!(
            Some(&true),
            seq.get(1).and_then(|a| a.downcast_ref::<bool>())
        );
        assert_eq!(
            Some(&456u16),
            seq.get(2).and_then(|a| a.downcast_ref::<u16>())
        );
        assert_eq!(
            Some(&"Hallo Welt!"),
            seq.get(3).and_then(|a| a.downcast_ref::<&str>())
        );
        assert!(seq.get(4).is_none());
    }

    #[test]
    fn test_placement_macro() {
        let mut seq: DynSequence<dyn Any> = DynSequence::new();
        dyn_sequence![dyn Any | &mut seq => {
            push(123u8);
            push(456u16);
            insert(true) @ 1;
            push("Hallo Welt!");
        }];

        assert_eq!(
            Some(&123u8),
            seq.get(0).and_then(|a| a.downcast_ref::<u8>())
        );
        assert_eq!(None, seq.get(0).and_then(|a| a.downcast_ref::<u16>()));
        assert_eq!(
            Some(&true),
            seq.get(1).and_then(|a| a.downcast_ref::<bool>())
        );
        assert_eq!(
            Some(&456u16),
            seq.get(2).and_then(|a| a.downcast_ref::<u16>())
        );
        assert_eq!(
            Some(&"Hallo Welt!"),
            seq.get(3).and_then(|a| a.downcast_ref::<&str>())
        );
        assert!(seq.get(4).is_none());
    }

    #[cfg(feature = "unstable")]
    #[test]
    fn test_push_insert_unstable() {
        let mut seq: DynSequence<dyn Any> = DynSequence::new();
        seq.push(123u8);
        seq.push(456u16);
        seq.insert(1, true);
        seq.push("Hallo Welt!");

        assert_eq!(
            Some(&123u8),
            seq.get(0).and_then(|a| a.downcast_ref::<u8>())
        );
        assert_eq!(None, seq.get(0).and_then(|a| a.downcast_ref::<u16>()));
        assert_eq!(
            Some(&true),
            seq.get(1).and_then(|a| a.downcast_ref::<bool>())
        );
        assert_eq!(
            Some(&456u16),
            seq.get(2).and_then(|a| a.downcast_ref::<u16>())
        );
        assert_eq!(
            Some(&"Hallo Welt!"),
            seq.get(3).and_then(|a| a.downcast_ref::<&str>())
        );
        assert!(seq.get(4).is_none());
    }
}
