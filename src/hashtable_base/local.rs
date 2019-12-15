use super::AlreadyFull;
use super::Key;
use super::TableEntry;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::ptr::drop_in_place;

pub struct LocalTableEntry<Value: 'static> {
    key000: UnsafeCell<Option<NonZeroU32>>,
    key001: UnsafeCell<NonZeroU32>,
    key01: UnsafeCell<[NonZeroU32; 2]>,
    key1: UnsafeCell<[[NonZeroU32; 2]; 2]>,
    value: UnsafeCell<MaybeUninit<Value>>,
}

impl<Value> LocalTableEntry<Value> {
    pub const fn empty() -> Self {
        unsafe {
            Self {
                key000: UnsafeCell::new(None),
                key001: UnsafeCell::new(NonZeroU32::new_unchecked(1)),
                key01: UnsafeCell::new([NonZeroU32::new_unchecked(1); 2]),
                key1: UnsafeCell::new([[NonZeroU32::new_unchecked(1); 2]; 2]),
                value: UnsafeCell::new(MaybeUninit::uninit()),
            }
        }
    }
    /// safety: self.value must not be concurrently accessed by any other threads
    unsafe fn get_value_mut_ptr(&self) -> *mut Value {
        (*self.value.get()).as_mut_ptr()
    }
    /// safety: self.value must not be concurrently written by any other threads
    unsafe fn get_value_ptr(&self) -> *const Value {
        (*self.value.get()).as_ptr()
    }
}

impl<Value> Drop for LocalTableEntry<Value> {
    fn drop(&mut self) {
        unsafe {
            if (&*self.key000.get()).is_some() {
                drop_in_place(self.get_value_mut_ptr());
            }
        }
    }
}

impl<Value> TableEntry for LocalTableEntry<Value> {
    type Value = Value;
    fn empty() -> Self {
        LocalTableEntry::empty()
    }
    fn get(&self) -> Option<(Key, &Self::Value)> {
        unsafe {
            let key000 = (*self.key000.get())?;
            let key001 = *self.key001.get();
            let key01 = *self.key01.get();
            let key1 = *self.key1.get();
            let value_ref = &*self.get_value_ptr();
            Some((Key([[[key000, key001], key01], key1]), value_ref))
        }
    }
    fn fill(&self, key: Key, value: Self::Value) -> Result<&Self::Value, AlreadyFull<Self::Value>> {
        if let Some((entry_key, entry_value)) = self.get() {
            Err(AlreadyFull {
                passed_in_value: value,
                entry_key,
                entry_value,
            })
        } else {
            let [[[key000, key001], key01], key1] = key.0;
            unsafe {
                *self.key000.get() = Some(key000);
                *self.key001.get() = key001;
                *self.key01.get() = key01;
                *self.key1.get() = key1;
                std::ptr::write(self.get_value_mut_ptr(), value);
                Ok(&*self.get_value_ptr())
            }
        }
    }
    fn take(&mut self) -> Option<(Key, Self::Value)> {
        unsafe {
            let key000 = ((&mut *self.key000.get()).take())?;
            let key001 = *self.key001.get();
            let key01 = *self.key01.get();
            let key1 = *self.key1.get();
            let value = std::ptr::read(self.get_value_mut_ptr());
            Some((Key([[[key000, key001], key01], key1]), value))
        }
    }
}
