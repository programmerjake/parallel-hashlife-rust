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
pub struct LateValueAlreadySet<'a, LateValue> {
    pub passed_in_value: LateValue,
    pub value: &'a LateValue,
}

#[derive(Debug)]
pub struct AlreadyFull<'a, Value: 'static> {
    pub passed_in_value: Value,
    pub entry_key: Key,
    pub entry_value: &'a Value,
}

pub trait TableEntryValues:
    Into<(
        <Self as TableEntryValues>::EarlyValue,
        Option<<Self as TableEntryValues>::LateValue>,
    )> + 'static
{
    type LateValue: Copy + 'static;
    type EarlyValue: Sized + 'static;
    fn new(early_value: Self::EarlyValue, late_value: Option<Self::LateValue>) -> Self;
    fn early_value(&self) -> &Self::EarlyValue;
    fn late_value(&self) -> Option<Self::LateValue>;
    fn set_late_value(&self, late_value: Option<Self::LateValue>);
}

pub trait TableEntry {
    type Values: TableEntryValues;
    fn empty() -> Self;
    fn get(&self) -> Option<(Key, &Self::Values)>;
    fn fill(
        &self,
        key: Key,
        value: Self::Values,
    ) -> Result<&Self::Values, AlreadyFull<Self::Values>>;
    fn take(&mut self) -> Option<(Key, Self::Values)>;
}

pub struct HashTable<Entry: TableEntry, BH: BuildHasher> {
    table: Option<Box<[Entry]>>,
    hasher: BH,
    insert_search_limit: usize,
}

#[derive(Debug)]
pub enum InsertFailureReason<'a, Value> {
    AlreadyInTable {
        passed_in_value: Value,
        entry_value: &'a Value,
    },
    TableFullOrSearchLimitHit {
        passed_in_value: Value,
    },
}

#[derive(Debug)]
pub struct GetOrInsertSuccess<'a, Value> {
    passed_in_value: Option<Value>,
    entry_value: &'a Value,
}

#[derive(Debug)]
pub enum GetOrInsertFailureReason<Value> {
    TableFullOrSearchLimitHit { passed_in_value: Value },
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

impl<Entry: TableEntry> Iterator for HashTableDrain<'_, Entry> {
    type Item = (Key, Entry::Values);
    fn next(&mut self) -> Option<(Key, Entry::Values)> {
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

impl<'a, Entry: TableEntry> Iterator for HashTableIter<'a, Entry> {
    type Item = (Key, &'a Entry::Values);
    fn next(&mut self) -> Option<(Key, &'a Entry::Values)> {
        self.entry_iter.next().and_then(TableEntry::get)
    }
}

impl<Entry: TableEntry, BH: BuildHasher> HashTable<Entry, BH> {
    pub fn with_search_limit_and_hasher(
        mut capacity: usize,
        insert_search_limit: usize,
        hasher: BH,
    ) -> Self {
        capacity = capacity
            .checked_next_power_of_two()
            .expect("capacity too big");
        Self {
            table: Some((0..capacity).map(|_| Entry::empty()).collect()),
            hasher,
            insert_search_limit,
        }
    }
    pub fn with_hasher(capacity: usize, hasher: BH) -> Self {
        Self::with_search_limit_and_hasher(capacity, 32, hasher)
    }
    pub fn with_search_limit(capacity: usize, insert_search_limit: usize) -> Self
    where
        BH: Default,
    {
        Self::with_search_limit_and_hasher(capacity, insert_search_limit, BH::default())
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
    pub fn insert_search_limit(&self) -> usize {
        self.insert_search_limit
    }
    pub fn set_insert_search_limit(&mut self, insert_search_limit: usize) {
        self.insert_search_limit = insert_search_limit;
    }
    fn table_indexes(&self, key: Key, limit: usize) -> impl Iterator<Item = usize> {
        let mut hasher = self.hasher.build_hasher();
        key.hash(&mut hasher);
        let table_index_mask = self.capacity() - 1;
        let table_index = hasher.finish() as usize & table_index_mask;
        TableIndexIter {
            table_index,
            table_index_mask,
        }
        .take(self.capacity().min(limit))
    }
    pub fn find(&self, key: Key) -> Option<&Entry::Values> {
        let table = self.get_table();
        for table_index in self.table_indexes(key, usize::max_value()) {
            let (entry_key, entry_value) = table[table_index].get()?;
            if entry_key == key {
                return Some(entry_value);
            }
        }
        None
    }
    pub fn insert(
        &self,
        key: Key,
        mut value: Entry::Values,
    ) -> Result<&Entry::Values, InsertFailureReason<Entry::Values>> {
        let table = self.get_table();
        for table_index in self.table_indexes(key, self.insert_search_limit) {
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
        Err(InsertFailureReason::TableFullOrSearchLimitHit {
            passed_in_value: value,
        })
    }
    pub fn get_or_insert(
        &self,
        key: Key,
        value: Entry::Values,
    ) -> Result<GetOrInsertSuccess<Entry::Values>, GetOrInsertFailureReason<Entry::Values>> {
        match self.insert(key, value) {
            Ok(entry_value) => Ok(GetOrInsertSuccess {
                entry_value,
                passed_in_value: None,
            }),
            Err(InsertFailureReason::AlreadyInTable {
                entry_value,
                passed_in_value,
            }) => Ok(GetOrInsertSuccess {
                entry_value,
                passed_in_value: Some(passed_in_value),
            }),
            Err(InsertFailureReason::TableFullOrSearchLimitHit { passed_in_value }) => {
                Err(GetOrInsertFailureReason::TableFullOrSearchLimitHit { passed_in_value })
            }
        }
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
        test_table_entry::<SyncTableEntry<DropCounter, NonZeroU32>>()
    }

    #[test]
    fn test_local_table_entry() {
        test_table_entry::<LocalTableEntry<DropCounter, NonZeroU32>>()
    }

    fn test_table_entry<T: TableEntry>()
    where
        T::Values: TableEntryValues<EarlyValue = DropCounter, LateValue = NonZeroU32>,
    {
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
                T::Values::new(
                    DropCounter {
                        drop_count: drop_count.clone(),
                    },
                    None,
                ),
            )
            .ok()
            .unwrap()
            .early_value();
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        let load1_result = table_entry.get().unwrap();
        assert_eq!(load1_result.0, key);
        assert_eq!(load1_result.1.early_value() as *const _, fill1_result);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        let drop_count2 = Arc::new(AtomicUsize::new(0));
        let fill2_result = table_entry
            .fill(
                key,
                T::Values::new(
                    DropCounter {
                        drop_count: drop_count2.clone(),
                    },
                    None,
                ),
            )
            .err()
            .unwrap();
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 0);
        std::mem::drop(fill2_result.passed_in_value);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        assert_eq!(fill2_result.entry_key, key);
        assert_eq!(
            fill2_result.entry_value.early_value() as *const _,
            fill1_result
        );
        let load2_result = table_entry.get().unwrap();
        assert_eq!(load2_result.0, key);
        assert_eq!(load2_result.1.early_value() as *const _, fill1_result);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        std::mem::drop(table_entry);
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count2.load(Ordering::Relaxed), 1);
        table_entry = TableEntry::empty();
        table_entry
            .fill(
                key,
                T::Values::new(
                    DropCounter {
                        drop_count: drop_count.clone(),
                    },
                    None,
                ),
            )
            .ok()
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
