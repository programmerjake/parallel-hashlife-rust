use super::AlreadyFull;
use super::Key;
use super::TableEntry;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::ptr::drop_in_place;
use std::sync::atomic::spin_loop_hint;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub struct SyncTableEntry<Value: 'static> {
    state: AtomicU64,
    key01: UnsafeCell<[NonZeroU32; 2]>,
    key1: UnsafeCell<[[NonZeroU32; 2]; 2]>,
    value: UnsafeCell<MaybeUninit<Value>>,
}

unsafe impl<Value: Sync> Sync for SyncTableEntry<Value> {}

impl<Value> Drop for SyncTableEntry<Value> {
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

impl<Value> SyncTableEntry<Value> {
    pub const fn empty() -> Self {
        unsafe {
            Self {
                state: AtomicU64::new(0),
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

impl<Value> TableEntry for SyncTableEntry<Value> {
    type Value = Value;
    fn empty() -> Self {
        SyncTableEntry::empty()
    }
    fn get(&self) -> Option<(Key, &Self::Value)> {
        let key00 = loop {
            match State::from(self.state.load(Ordering::Acquire)) {
                State::Empty => return None,
                State::Full { key00 } => break key00,
                State::ModificationInProgress => spin_loop_hint(),
            }
        };
        // safety: state will never transition from Full to something else while self is shared
        unsafe {
            let key01 = *self.key01.get();
            let key1 = *self.key1.get();
            Some((Key([[key00, key01], key1]), &*self.get_value_ptr()))
        }
    }
    fn fill(&self, key: Key, value: Self::Value) -> Result<&Self::Value, AlreadyFull<Self::Value>> {
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
    fn take(&mut self) -> Option<(Key, Self::Value)> {
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
