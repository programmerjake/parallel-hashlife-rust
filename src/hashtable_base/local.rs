use crate::hashtable_base::AlreadyFull;
use crate::hashtable_base::Key;
use crate::hashtable_base::TableEntry;
use crate::hashtable_base::TableEntryValues;
use std::cell::Cell;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::ptr::drop_in_place;

pub struct LocalTableValues<EarlyValue: 'static, LateValue: Copy + 'static> {
    early_value: EarlyValue,
    late_value: Cell<Option<LateValue>>,
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> TableEntryValues
    for LocalTableValues<EarlyValue, LateValue>
{
    type LateValue = LateValue;
    type EarlyValue = EarlyValue;
    fn new(early_value: Self::EarlyValue, late_value: Option<Self::LateValue>) -> Self {
        Self {
            early_value,
            late_value: Cell::new(late_value),
        }
    }
    fn early_value(&self) -> &Self::EarlyValue {
        &self.early_value
    }
    fn late_value(&self) -> Option<Self::LateValue> {
        self.late_value.get()
    }
    fn set_late_value(&self, late_value: Option<Self::LateValue>) {
        self.late_value.set(late_value);
    }
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> Into<(EarlyValue, Option<LateValue>)>
    for LocalTableValues<EarlyValue, LateValue>
{
    fn into(self) -> (EarlyValue, Option<LateValue>) {
        let Self {
            early_value,
            late_value,
        } = self;
        (early_value, late_value.into_inner())
    }
}

pub struct LocalTableEntry<EarlyValue: 'static, LateValue: Copy + 'static> {
    key000: UnsafeCell<Option<NonZeroU32>>,
    key001: UnsafeCell<NonZeroU32>,
    key01: UnsafeCell<[NonZeroU32; 2]>,
    key1: UnsafeCell<[[NonZeroU32; 2]; 2]>,
    value: UnsafeCell<MaybeUninit<LocalTableValues<EarlyValue, LateValue>>>,
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> LocalTableEntry<EarlyValue, LateValue> {
    pub const EMPTY: Self = unsafe {
        Self {
            key000: UnsafeCell::new(None),
            key001: UnsafeCell::new(NonZeroU32::new_unchecked(1)),
            key01: UnsafeCell::new([NonZeroU32::new_unchecked(1); 2]),
            key1: UnsafeCell::new([[NonZeroU32::new_unchecked(1); 2]; 2]),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    };
    /// safety: self.value must not be concurrently accessed by any other threads
    unsafe fn get_value_mut_ptr(&self) -> *mut LocalTableValues<EarlyValue, LateValue> {
        (*self.value.get()).as_mut_ptr()
    }
    /// safety: self.value must not be concurrently written by any other threads
    unsafe fn get_value_ptr(&self) -> *const LocalTableValues<EarlyValue, LateValue> {
        (*self.value.get()).as_ptr()
    }
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> Drop
    for LocalTableEntry<EarlyValue, LateValue>
{
    fn drop(&mut self) {
        unsafe {
            if (&*self.key000.get()).is_some() {
                drop_in_place(self.get_value_mut_ptr());
            }
        }
    }
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> TableEntry
    for LocalTableEntry<EarlyValue, LateValue>
{
    type Values = LocalTableValues<EarlyValue, LateValue>;
    fn empty() -> Self {
        LocalTableEntry::EMPTY
    }
    fn get(&self) -> Option<(Key, &Self::Values)> {
        unsafe {
            let key000 = (*self.key000.get())?;
            let key001 = *self.key001.get();
            let key01 = *self.key01.get();
            let key1 = *self.key1.get();
            let value_ref = &*self.get_value_ptr();
            Some((Key([[[key000, key001], key01], key1]), value_ref))
        }
    }
    fn fill(
        &self,
        key: Key,
        value: Self::Values,
    ) -> Result<&Self::Values, AlreadyFull<Self::Values>> {
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
    fn take(&mut self) -> Option<(Key, Self::Values)> {
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
