pub use crate::hashtable_base::GetOrInsertFailureReason;
pub use crate::hashtable_base::GetOrInsertSuccess;
pub use crate::hashtable_base::InsertFailureReason;
use crate::hashtable_base::{
    HashTable as BaseHashTable, HashTableIter as BaseHashTableIter, Key as BaseKey, TableEntry,
};
use std::fmt;
use std::hash::BuildHasher;
use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZeroU32;

pub struct HashTables<Value: Sized + 'static, Entry: TableEntry<Value>, BH: BuildHasher> {
    hash_tables: Vec<BaseHashTable<Value, Entry, BH>>,
}

impl<Value: Sized + 'static, Entry: TableEntry<Value>, BH: BuildHasher>
    HashTables<Value, Entry, BH>
{
    pub fn get<L: Level>(&self) -> HashTableRef<L, Value, Entry, BH> {
        HashTableRef::new(&self.hash_tables[L::LEVEL])
    }
}

pub trait Level: 'static + Copy + Eq + Hash + fmt::Debug {
    const LEVEL: usize;
}

pub trait NonLeafLevel: Level {
    type ParentLevel: Level;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Leaf(PhantomData<()>);

impl Level for Leaf {
    const LEVEL: usize = 0;
}

impl<L: Level> Level for NonLeaf<L> {
    const LEVEL: usize = 1 + L::LEVEL;
}

impl<L: Level> NonLeafLevel for NonLeaf<L> {
    type ParentLevel = L;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NonLeaf<L: Level>(PhantomData<L>);

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
#[repr(transparent)]
pub struct Id<L: Level> {
    id: NonZeroU32,
    _phantom: PhantomData<L>,
}

impl<L: Level> fmt::Debug for Id<L> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Id { id, _phantom } = self;
        f.debug_struct("Id").field("id", id).finish()
    }
}

impl<L: Level> From<NonZeroU32> for Id<L> {
    fn from(id: NonZeroU32) -> Id<L> {
        Id {
            id,
            _phantom: PhantomData,
        }
    }
}

impl<L: Level> From<Id<L>> for NonZeroU32 {
    fn from(v: Id<L>) -> NonZeroU32 {
        v.id
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Key<L: Level>(pub [[[Id<L>; 2]; 2]; 2]);

impl<L: Level> From<Key<L>> for BaseKey {
    fn from(v: Key<L>) -> BaseKey {
        let [[[v000, v001], [v010, v011]], [[v100, v101], [v110, v111]]] = v.0;
        BaseKey([
            [[v000.into(), v001.into()], [v010.into(), v011.into()]],
            [[v100.into(), v101.into()], [v110.into(), v111.into()]],
        ])
    }
}

impl<L: Level> From<BaseKey> for Key<L> {
    fn from(v: BaseKey) -> Key<L> {
        let [[[v000, v001], [v010, v011]], [[v100, v101], [v110, v111]]] = v.0;
        Key([
            [[v000.into(), v001.into()], [v010.into(), v011.into()]],
            [[v100.into(), v101.into()], [v110.into(), v111.into()]],
        ])
    }
}

pub struct HashTableIter<'a, L: Level, Value: Sized + 'static, Entry: TableEntry<Value>> {
    base: BaseHashTableIter<'a, Value, Entry>,
    _phantom: PhantomData<L>,
}

impl<'a, L: Level, Value: Sized + 'static, Entry: TableEntry<Value>> Iterator
    for HashTableIter<'a, L, Value, Entry>
{
    type Item = (Key<L>, &'a Value);
    fn next(&mut self) -> Option<(Key<L>, &'a Value)> {
        let (key, value) = self.base.next()?;
        Some((key.into(), value))
    }
}

pub struct HashTableRef<
    'a,
    L: Level,
    Value: Sized + 'static,
    Entry: TableEntry<Value>,
    BH: BuildHasher,
> {
    base: &'a BaseHashTable<Value, Entry, BH>,
    _phantom: PhantomData<L>,
}

impl<'a, L: Level, Value: Sized + 'static, Entry: TableEntry<Value>, BH: BuildHasher>
    HashTableRef<'a, L, Value, Entry, BH>
{
    pub fn new(base: &'a BaseHashTable<Value, Entry, BH>) -> Self {
        Self {
            base,
            _phantom: PhantomData,
        }
    }
    pub fn capacity(&self) -> usize {
        self.base.capacity()
    }
    pub fn hasher(&self) -> &BH {
        self.base.hasher()
    }
    pub fn insert_search_limit(&self) -> usize {
        self.base.insert_search_limit()
    }
    pub fn find(&self, key: Key<L>) -> Option<&Value> {
        self.base.find(key.into())
    }
    pub fn insert(&self, key: Key<L>, value: Value) -> Result<&Value, InsertFailureReason<Value>> {
        self.base.insert(key.into(), value)
    }
    pub fn get_or_insert(
        &self,
        key: Key<L>,
        value: Value,
    ) -> Result<GetOrInsertSuccess<Value>, GetOrInsertFailureReason<Value>> {
        self.base.get_or_insert(key.into(), value)
    }
    pub fn iter(&self) -> HashTableIter<L, Value, Entry> {
        HashTableIter {
            base: self.base.iter(),
            _phantom: PhantomData,
        }
    }
}
