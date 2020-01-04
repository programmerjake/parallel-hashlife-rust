pub use crate::hashtable_base::GetOrInsertFailureReason;
pub use crate::hashtable_base::GetOrInsertSuccess;
pub use crate::hashtable_base::InsertFailureReason;
pub use crate::hashtable_base::TableEntry;
use crate::hashtable_base::{
    HashTable as BaseHashTable, Key as BaseKey, TableEntryValues as TableEntryValuesBase,
};
use std::fmt;
use std::hash::BuildHasher;
use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZeroU32;

pub struct HashTables<Entry: TableEntry, BH: BuildHasher> {
    hash_tables: Vec<BaseHashTable<Entry, BH>>,
}

impl<Entry: TableEntry, BH: BuildHasher> HashTables<Entry, BH>
where
    Entry::Values: TableEntryValuesBase<LateValue = NonZeroU32>,
{
    pub fn get<L: Level>(
        &self,
    ) -> &impl HashTable<
        L,
        EarlyValue = <Entry::Values as TableEntryValues<L>>::EarlyValue,
        LateValue = <Entry::Values as TableEntryValues<L>>::LateValue,
    > {
        &self.hash_tables[L::LEVEL]
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

pub type Level1 = NonLeaf<Leaf>;
pub type Level2 = NonLeaf<Level1>;
pub type Level3 = NonLeaf<Level2>;

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

pub trait TableEntryValues<L: Level> {
    type LateValue: Copy + 'static;
    type EarlyValue: Sized + 'static;
    fn new(early_value: Self::EarlyValue, late_value: Option<Self::LateValue>) -> Self;
    fn early_value(&self) -> &Self::EarlyValue;
    fn late_value(&self) -> Option<Self::LateValue>;
    fn set_late_value(&self, late_value: Option<Self::LateValue>);
    fn into(self) -> (Self::EarlyValue, Option<Self::LateValue>);
}

impl<L: Level, Entry: TableEntryValuesBase<LateValue = NonZeroU32>> TableEntryValues<L> for Entry {
    type EarlyValue = Entry::EarlyValue;
    type LateValue = Id<L>;
    fn new(early_value: Self::EarlyValue, late_value: Option<Self::LateValue>) -> Self {
        TableEntryValuesBase::new(early_value, late_value.map(Into::into))
    }
    fn early_value(&self) -> &Self::EarlyValue {
        TableEntryValuesBase::early_value(self)
    }
    fn late_value(&self) -> Option<Self::LateValue> {
        TableEntryValuesBase::late_value(self).map(Into::into)
    }
    fn set_late_value(&self, late_value: Option<Self::LateValue>) {
        TableEntryValuesBase::set_late_value(self, late_value.map(Into::into));
    }
    fn into(self) -> (Self::EarlyValue, Option<Self::LateValue>) {
        let (early_value, late_value) = Into::into(self);
        (early_value, late_value.map(Into::into))
    }
}

pub trait HashTable<L: Level> {
    type LateValue: Copy + 'static;
    type EarlyValue: Sized + 'static;
    type Values: TableEntryValues<L, EarlyValue = Self::EarlyValue, LateValue = Self::LateValue>;
    fn capacity(&self) -> usize;
    fn insert_search_limit(&self) -> usize;
    fn find(&self, key: Key<L>) -> Option<&Self::Values>;
    fn insert(
        &self,
        key: Key<L>,
        value: Self::Values,
    ) -> Result<&Self::Values, InsertFailureReason<Self::Values>>;
    fn get_or_insert(
        &self,
        key: Key<L>,
        value: Self::Values,
    ) -> Result<GetOrInsertSuccess<Self::Values>, GetOrInsertFailureReason<Self::Values>>;
}

impl<L: Level, Entry: TableEntry, BH: BuildHasher> HashTable<L> for BaseHashTable<Entry, BH>
where
    Entry::Values: TableEntryValues<L>,
{
    type EarlyValue = <Entry::Values as TableEntryValues<L>>::EarlyValue;
    type LateValue = <Entry::Values as TableEntryValues<L>>::LateValue;
    type Values = Entry::Values;
    fn capacity(&self) -> usize {
        BaseHashTable::capacity(self)
    }
    fn insert_search_limit(&self) -> usize {
        BaseHashTable::insert_search_limit(self)
    }
    fn find(&self, key: Key<L>) -> Option<&Self::Values> {
        BaseHashTable::find(self, key.into())
    }
    fn insert(
        &self,
        key: Key<L>,
        value: Self::Values,
    ) -> Result<&Self::Values, InsertFailureReason<Self::Values>> {
        BaseHashTable::insert(self, key.into(), value)
    }
    fn get_or_insert(
        &self,
        key: Key<L>,
        value: Self::Values,
    ) -> Result<GetOrInsertSuccess<Self::Values>, GetOrInsertFailureReason<Self::Values>> {
        BaseHashTable::get_or_insert(self, key.into(), value)
    }
}
