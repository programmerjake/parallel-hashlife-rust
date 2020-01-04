use crate::hashtable_base::AlreadyFull;
use crate::hashtable_base::Key;
use crate::hashtable_base::TableEntry;
use crate::hashtable_base::TableEntryValues;
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::ptr::drop_in_place;
use std::sync::atomic::spin_loop_hint;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// `LateValue` must be `NonZeroU32`
pub struct SyncTableValues<EarlyValue: 'static, LateValue: Copy + 'static> {
    early_value: EarlyValue,
    late_value: AtomicU32,
    _phantom: PhantomData<LateValue>,
}

impl<EarlyValue: 'static> TableEntryValues for SyncTableValues<EarlyValue, NonZeroU32> {
    type LateValue = NonZeroU32;
    type EarlyValue = EarlyValue;
    fn new(early_value: Self::EarlyValue, late_value: Option<Self::LateValue>) -> Self {
        Self {
            early_value,
            late_value: AtomicU32::new(late_value.map(NonZeroU32::get).unwrap_or(0)),
            _phantom: PhantomData,
        }
    }
    fn early_value(&self) -> &Self::EarlyValue {
        &self.early_value
    }
    fn late_value(&self) -> Option<Self::LateValue> {
        NonZeroU32::new(self.late_value.load(Ordering::Acquire))
    }
    fn set_late_value(&self, late_value: Option<Self::LateValue>) {
        self.late_value.store(
            late_value.map(NonZeroU32::get).unwrap_or(0),
            Ordering::Release,
        );
    }
}

impl<EarlyValue: 'static> Into<(EarlyValue, Option<NonZeroU32>)>
    for SyncTableValues<EarlyValue, NonZeroU32>
{
    fn into(self) -> (EarlyValue, Option<NonZeroU32>) {
        let Self {
            early_value,
            late_value,
            _phantom,
        } = self;
        (early_value, NonZeroU32::new(late_value.into_inner()))
    }
}

pub struct SyncTableEntry<EarlyValue: 'static, LateValue: Copy + 'static> {
    state: AtomicU64,
    key01: UnsafeCell<[NonZeroU32; 2]>,
    key1: UnsafeCell<[[NonZeroU32; 2]; 2]>,
    value: UnsafeCell<MaybeUninit<SyncTableValues<EarlyValue, LateValue>>>,
}

unsafe impl<EarlyValue: 'static + Send + Sync, LateValue: Copy + 'static + Send + Sync> Sync
    for SyncTableEntry<EarlyValue, LateValue>
{
}

impl<EarlyValue, LateValue: Copy> Drop for SyncTableEntry<EarlyValue, LateValue> {
    fn drop(&mut self) {
        match State::from(*self.state.get_mut()) {
            State::Empty => {}
            State::ModificationInProgress => unreachable!("invalid state"),
            State::Full { .. } => unsafe { drop_in_place(self.get_value_mut_ptr()) },
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum State {
    Empty,
    ModificationInProgress,
    Full { key00: [NonZeroU32; 2] },
}

#[cfg(target_endian = "big")]
const fn unpack_u64(v: u64) -> [u32; 2] {
    [(v >> 32) as u32, v as u32]
}

#[cfg(not(target_endian = "big"))]
const fn unpack_u64(v: u64) -> [u32; 2] {
    [v as u32, (v >> 32) as u32]
}

#[cfg(target_endian = "big")]
const fn pack_u64(v: [u32; 2]) -> u64 {
    let [v0, v1] = v;
    ((v0 as u64) << 32) + v1 as u64
}

#[cfg(not(target_endian = "big"))]
const fn pack_u64(v: [u32; 2]) -> u64 {
    let [v0, v1] = v;
    v0 as u64 + ((v1 as u64) << 32)
}

impl State {
    const EMPTY_U64: u64 = pack_u64([0, 0]);
    const MODIFICATION_IN_PROGRESS_U64: u64 = pack_u64([1, 0]);
}

impl From<State> for u64 {
    fn from(v: State) -> u64 {
        match v {
            State::Empty => State::EMPTY_U64,
            State::ModificationInProgress => State::MODIFICATION_IN_PROGRESS_U64,
            State::Full { key00: [u0, u1] } => pack_u64([u0.get(), u1.get()]),
        }
    }
}

impl From<u64> for State {
    fn from(v: u64) -> State {
        let [u0, u1] = unpack_u64(v);
        match v {
            State::EMPTY_U64 => State::Empty,
            State::MODIFICATION_IN_PROGRESS_U64 => State::ModificationInProgress,
            _ => State::Full {
                key00: [
                    NonZeroU32::new(u0).expect("invalid state"),
                    NonZeroU32::new(u1).expect("invalid state"),
                ],
            },
        }
    }
}

impl<EarlyValue: 'static, LateValue: 'static + Copy> SyncTableEntry<EarlyValue, LateValue> {
    pub const EMPTY: Self = unsafe {
        Self {
            state: AtomicU64::new(0),
            key01: UnsafeCell::new([NonZeroU32::new_unchecked(1); 2]),
            key1: UnsafeCell::new([[NonZeroU32::new_unchecked(1); 2]; 2]),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    };
    /// safety: self.value must not be concurrently accessed by any other threads
    unsafe fn get_value_mut_ptr(&self) -> *mut SyncTableValues<EarlyValue, LateValue> {
        (*self.value.get()).as_mut_ptr()
    }
    /// safety: self.value must not be concurrently written by any other threads
    unsafe fn get_value_ptr(&self) -> *const SyncTableValues<EarlyValue, LateValue> {
        (*self.value.get()).as_ptr()
    }
}

impl<EarlyValue: 'static, LateValue: Copy + 'static> TableEntry
    for SyncTableEntry<EarlyValue, LateValue>
where
    SyncTableValues<EarlyValue, LateValue>:
        TableEntryValues<EarlyValue = EarlyValue, LateValue = LateValue>,
{
    type Values = SyncTableValues<EarlyValue, LateValue>;
    fn empty() -> Self {
        SyncTableEntry::EMPTY
    }
    fn get(&self) -> Option<(Key, &Self::Values)> {
        let mut backoff_step = 0;
        let key00 = loop {
            match State::from(self.state.load(Ordering::Acquire)) {
                State::Empty => return None,
                State::Full { key00 } => break key00,
                State::ModificationInProgress => {
                    if backoff_step <= 6 {
                        for _ in 0..(1 << backoff_step) {
                            spin_loop_hint()
                        }
                        backoff_step += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            }
        };
        // safety: state will never transition from Full to something else while self is shared
        unsafe {
            let key01 = *self.key01.get();
            let key1 = *self.key1.get();
            Some((Key([[key00, key01], key1]), &*self.get_value_ptr()))
        }
    }
    fn fill(
        &self,
        key: Key,
        value: Self::Values,
    ) -> Result<&Self::Values, AlreadyFull<Self::Values>> {
        loop {
            match self
                .state
                .compare_exchange_weak(
                    State::EMPTY_U64,
                    State::MODIFICATION_IN_PROGRESS_U64,
                    Ordering::Acquire,
                    Ordering::Acquire,
                )
                .map_err(State::from)
            {
                Ok(_) => break,
                Err(State::Empty) => {
                    // spurious failure; try again
                }
                Err(State::ModificationInProgress) => {
                    // another thread is filling self

                    // get waits for modification to finish
                    let (entry_key, entry_value) = self.get().expect("invalid state");

                    return Err(AlreadyFull {
                        passed_in_value: value,
                        entry_key,
                        entry_value,
                    });
                }
                Err(State::Full { key00 }) => unsafe {
                    let key01 = *self.key01.get();
                    let key1 = *self.key1.get();
                    let entry_key = Key([[key00, key01], key1]);
                    return Err(AlreadyFull {
                        passed_in_value: value,
                        entry_key,
                        entry_value: &*self.get_value_ptr(),
                    });
                },
            }
        }
        let [[key00, key01], key1] = key.0;
        // safety: state is currently ModificationInProgress, which will block all concurrent accesses until state is stored to
        unsafe {
            *self.key01.get() = key01;
            *self.key1.get() = key1;
            std::ptr::write(self.get_value_mut_ptr(), value);
            // finish modifying
            self.state
                .store(u64::from(State::Full { key00 }), Ordering::Release);
            Ok(&*self.get_value_ptr())
        }
    }
    fn take(&mut self) -> Option<(Key, Self::Values)> {
        unsafe {
            match State::from(*self.state.get_mut()) {
                State::Empty => None,
                State::ModificationInProgress => unreachable!("invalid state"),
                State::Full { key00 } => {
                    *self.state.get_mut() = State::Empty.into();
                    let key01 = *self.key01.get();
                    let key1 = *self.key1.get();
                    let value = std::ptr::read(self.get_value_mut_ptr());
                    Some((Key([[key00, key01], key1]), value))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack() {
        assert_eq!(unpack_u64(pack_u64([1, 2])), [1, 2]);
        assert_eq!(unpack_u64(pack_u64([12345, 2])), [12345, 2]);
        assert_eq!(unpack_u64(pack_u64([0, 5])), [0, 5]);
    }

    #[test]
    fn test_state() {
        assert_eq!(State::EMPTY_U64, 0);
        assert_ne!(State::MODIFICATION_IN_PROGRESS_U64, State::EMPTY_U64);
        let [u0, u1] = unpack_u64(State::EMPTY_U64);
        assert!(u0 == 0 || u1 == 0);
        let [u0, u1] = unpack_u64(State::MODIFICATION_IN_PROGRESS_U64);
        assert!(u0 == 0 || u1 == 0);
    }
}
