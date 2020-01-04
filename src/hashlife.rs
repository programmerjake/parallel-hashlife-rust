use crate::hashtable::{
    GetOrInsertFailureReason, HashTables, Id, Key, Leaf, Level, Level1, Level2, NonLeaf,
    NonLeafLevel, TableEntry,
};
use std::hash::BuildHasher;

#[derive(Debug)]
pub enum FailureReason {
    TableFullOrSearchLimitHit,
}

impl<T> From<GetOrInsertFailureReason<T>> for FailureReason {
    fn from(v: GetOrInsertFailureReason<T>) -> Self {
        match v {
            GetOrInsertFailureReason::TableFullOrSearchLimitHit { .. } => {
                FailureReason::TableFullOrSearchLimitHit
            }
        }
    }
}

macro_rules! parallel_for_2 {
    ($index:ident, $return_type:ty, $code:expr) => {
        join(
            || -> $return_type {
                let $index = 0;
                $code
            },
            || -> $return_type {
                let $index = 1;
                $code
            },
        )
    };
}

macro_rules! impl_hashlife {
    ($send:ident, $sync:ident, $mod:ident, $join:path) => {
        pub mod $mod {
            use super::*;
            use $join as join;

            pub trait StepBase: $sync {
                fn get_next_state(
                    &self,
                    state: [[[Id<Leaf>; 3]; 3]; 3],
                ) -> Result<Id<Leaf>, FailureReason>;
            }

            pub trait Step<L: NonLeafLevel, Entry: TableEntry, BH: BuildHasher>: StepBase {
                fn step(
                    &self,
                    hashtables: &HashTables<Entry, BH>,
                    key: Key<NonLeaf<L>>,
                ) -> Result<Key<L>, FailureReason>;
            }

            impl<T: StepBase, Entry: TableEntry, BH: BuildHasher> Step<Level1, Entry, BH> for T {
                fn step(
                    &self,
                    hashtables: &HashTables<Entry, BH>,
                    key: Key<Level2>,
                ) -> Result<Key<Level1>, FailureReason> {
                    for output_x in 0..2 {
                        for output_y in 0..2 {
                            for output_z in 0..2 {
                                for dx in 0..3 {
                                    for dy in 0..3 {
                                        for dz in 0..3 {}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            impl<T: StepBase, L: NonLeafLevel, Entry: TableEntry, BH: BuildHasher>
                Step<NonLeaf<L>, Entry, BH> for T
            {
                fn step(
                    &self,
                    hashtables: &HashTables<Entry, BH>,
                    key: Key<NonLeaf<NonLeaf<L>>>,
                ) -> Result<Key<NonLeaf<L>>, FailureReason> {
                    parallel_for_2!(output_x, _, {
                        parallel_for_2!(output_y, _, {
                            parallel_for_2!(output_z, _, {
                                for dx in 0..3 {
                                    for dy in 0..3 {
                                        for dz in 0..3 {}
                                    }
                                }
                            });
                        });
                    });
                    todo!()
                }
            }
        }
    };
}

pub trait FakeSend {}

impl<T: ?Sized> FakeSend for T {}

pub trait FakeSync {}

impl<T: ?Sized> FakeSync for T {}

fn sync_join<A, B, RA, RB>(a: A, b: B) -> (RA, RB)
where
    A: FnOnce() -> RA + Send,
    B: FnOnce() -> RB + Send,
    RA: Send,
    RB: Send,
{
    todo!()
}

fn local_join<A, B, RA, RB>(a: A, b: B) -> (RA, RB)
where
    A: FnOnce() -> RA + FakeSend,
    B: FnOnce() -> RB + FakeSend,
    RA: FakeSend,
    RB: FakeSend,
{
    let ra = a();
    (ra, b())
}

impl_hashlife!(FakeSend, FakeSync, local, local_join);
impl_hashlife!(Send, Sync, sync, sync_join);
