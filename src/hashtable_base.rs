use std::hash::BuildHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU32;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Key(pub [[[NonZeroU32; 2]; 2]; 2]);

mod local;
mod sync;

pub use local::LocalTableEntry;
pub use sync::SyncTableEntry;

#[derive(Debug)]
pub struct AlreadyFull<'a, Value: 'static> {
    pub passed_in_value: Value,
    pub entry_key: Key,
    pub entry_value: &'a Value,
}

pub trait TableEntry {
    type Value: Sized + 'static;
    fn empty() -> Self;
    fn get(&self) -> Option<(Key, &Self::Value)>;
    fn fill(&self, key: Key, value: Self::Value) -> Result<&Self::Value, AlreadyFull<Self::Value>>;
    fn take(&mut self) -> Option<(Key, Self::Value)>;
}

pub struct HashTable<Entry: TableEntry, BH: BuildHasher> {
    table: Option<Box<[Entry]>>,
    hasher: BH,
}

#[derive(Debug)]
pub enum InsertFailureReason<'a, Value> {
    AlreadyInTable {
        passed_in_value: Value,
        entry_value: &'a Value,
    },
    TableFull {
        passed_in_value: Value,
    },
}

struct TableIndexIter {
    table_index: usize,
    table_index_mask: usize,
}

impl Iterator for TableIndexIter {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        let retval = self.table_index;
        self.table_index = self.table_index.wrapping_add(1) & self.table_index_mask;
        Some(retval)
    }
}

pub struct HashTableDrain<'a, Entry: TableEntry> {
    entry_iter: std::slice::IterMut<'a, Entry>,
}

impl<Entry: TableEntry<Value = Value>, Value: 'static> Iterator for HashTableDrain<'_, Entry> {
    type Item = (Key, Value);
    fn next(&mut self) -> Option<(Key, Value)> {
        self.entry_iter.next().and_then(TableEntry::take)
    }
}

impl<Entry: TableEntry> Drop for HashTableDrain<'_, Entry> {
    fn drop(&mut self) {
        self.for_each(std::mem::drop);
    }
}

pub struct HashTableIter<'a, Entry: TableEntry> {
    entry_iter: std::slice::Iter<'a, Entry>,
}

impl<'a, Entry: TableEntry<Value = Value>, Value: 'static> Iterator for HashTableIter<'a, Entry> {
    type Item = (Key, &'a Value);
    fn next(&mut self) -> Option<(Key, &'a Value)> {
        self.entry_iter.next().and_then(TableEntry::get)
    }
}

impl<Entry: TableEntry<Value = Value>, BH: BuildHasher, Value: 'static> HashTable<Entry, BH> {
    pub fn with_hasher(mut capacity: usize, hasher: BH) -> Self {
        capacity = capacity
            .checked_next_power_of_two()
            .expect("capacity too big");
        Self {
            table: Some((0..capacity).map(|_| Entry::empty()).collect()),
            hasher,
        }
    }
    pub fn new(capacity: usize) -> Self
    where
        BH: Default,
    {
        Self::with_hasher(capacity, BH::default())
    }
    fn get_table(&self) -> &[Entry] {
        self.table.as_ref().expect("table is known to be Some")
    }
    fn get_table_mut(&mut self) -> &mut [Entry] {
        self.table.as_mut().expect("table is known to be Some")
    }
    pub fn capacity(&self) -> usize {
        self.get_table().len()
    }
    pub fn hasher(&self) -> &BH {
        &self.hasher
    }
    fn table_indexes(&self, key: Key) -> impl Iterator<Item = usize> {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        let table_index_mask = self.capacity() - 1;
        let table_index = hasher.finish() as usize & table_index_mask;
        TableIndexIter {
            table_index,
            table_index_mask,
        }
        .take(self.capacity())
    }
    pub fn find(&self, key: Key) -> Option<&Value> {
        let table = self.get_table();
        for table_index in self.table_indexes(key) {
            let (entry_key, entry_value) = table[table_index].get()?;
            if entry_key == key {
                return Some(entry_value);
            }
        }
        None
    }
    pub fn insert(&self, key: Key, mut value: Value) -> Result<&Value, InsertFailureReason<Value>> {
        let table = self.get_table();
        for table_index in self.table_indexes(key) {
            match table[table_index].fill(key, value) {
                Ok(entry_value) => return Ok(entry_value),
                Err(AlreadyFull {
                    passed_in_value,
                    entry_key,
                    entry_value,
                }) => {
                    if entry_key == key {
                        return Err(InsertFailureReason::AlreadyInTable {
                            entry_value,
                            passed_in_value,
                        });
                    }
                    value = passed_in_value;
                }
            }
        }
        Err(InsertFailureReason::TableFull {
            passed_in_value: value,
        })
    }
    pub fn drain(&mut self) -> HashTableDrain<Entry> {
        HashTableDrain {
            entry_iter: self.get_table_mut().iter_mut(),
        }
    }
    pub fn iter(&self) -> HashTableIter<Entry> {
        HashTableIter {
            entry_iter: self.get_table().iter(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    #[derive(Debug)]
    struct DropCounter {
        drop_count: Arc<AtomicUsize>,
    }

    impl Drop for DropCounter {
        fn drop(&mut self) {
            let value = self.drop_count.fetch_add(1, Ordering::Relaxed);
            println!("DropCounter::drop: {}", value);
        }
    }

    #[test]
    fn test_sync_table_entry() {
        test_table_entry::<SyncTableEntry<DropCounter>>()
    }

    #[test]
    fn test_local_table_entry() {
        test_table_entry::<LocalTableEntry<DropCounter>>()
    }

    fn test_table_entry<T: TableEntry<Value = DropCounter>>() {
        #![allow(clippy::cognitive_complexity)]
        let drop_count = Arc::new(AtomicUsize::new(0));
        let key = Key([
            [
                [NonZeroU32::new(9).unwrap(), NonZeroU32::new(2).unwrap()],
                [NonZeroU32::new(3).unwrap(), NonZeroU32::new(4).unwrap()],
            ],
            [
                [NonZeroU32::new(5).unwrap(), NonZeroU32::new(6).unwrap()],
                [NonZeroU32::new(7).unwrap(), NonZeroU32::new(8).unwrap()],
            ],
        ]);
        let mut table_entry = T::empty();
        assert!(table_entry.get().is_none());
        assert!(table_entry.take().is_none());
        let fill1_result = table_entry
            .fill(
                key,
                DropCounter {
                    drop_count: drop_count.clone(),
                },
            )
            .unwrap();
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        let load1_result = table_entry.get().unwrap();
        assert_eq!(load1_result.0, key);
        assert_eq!(load1_result.1 as *const _, fill1_result);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        let drop_count2 = Arc::new(AtomicUsize::new(0));
        let fill2_result = table_entry
            .fill(
                key,
                DropCounter {
                    drop_count: drop_count2.clone(),
                },
            )
            .unwrap_err();
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 0);
        std::mem::drop(fill2_result.passed_in_value);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        assert_eq!(fill2_result.entry_key, key);
        assert_eq!(fill2_result.entry_value as *const _, fill1_result);
        let load2_result = table_entry.get().unwrap();
        assert_eq!(load2_result.0, key);
        assert_eq!(load2_result.1 as *const _, fill1_result);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        std::mem::drop(table_entry);
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        table_entry = TableEntry::empty();
        table_entry
            .fill(
                key,
                DropCounter {
                    drop_count: drop_count.clone(),
                },
            )
            .unwrap();
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
        let take_result = table_entry.take().unwrap();
        assert_eq!(take_result.0, key);
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
        std::mem::drop(take_result.1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 2);
        assert!(table_entry.get().is_none());
        assert!(table_entry.take().is_none());
        assert_eq!(drop_count.load(Ordering::Relaxed), 2);
        std::mem::drop(table_entry);
        assert_eq!(drop_count.load(Ordering::Relaxed), 2);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
    }
}