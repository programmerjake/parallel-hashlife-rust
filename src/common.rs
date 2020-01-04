use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::num::NonZeroU32;

pub trait Level: 'static + Copy + Eq + Hash + fmt::Debug {
    const LEVEL: usize;
}

pub trait NonZeroLevel: Level {
    type ParentLevel: Level;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Level0(PhantomData<()>);

impl Level for Level0 {
    const LEVEL: usize = 0;
}

impl<L: Level> Level for NextLevel<L> {
    const LEVEL: usize = 1 + L::LEVEL;
}

impl<L: Level> NonZeroLevel for NextLevel<L> {
    type ParentLevel = L;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NextLevel<L: Level>(PhantomData<L>);

macro_rules! define_levels {
    ([$first_level:ident, $($level:ident,)+]) => {
        define_levels!([$first_level, $($level,)+], [$($level,)+]);
    };
    ([$level0:ident, $($levels0:ident,)+], [$level1:ident, $($levels1:ident,)*]) => {
        pub type $level1 = NextLevel<$level0>;
        define_levels!([$($levels0,)+], [$($levels1,)*]);
    };
    ([$level:ident,], []) => {};
}

define_levels!([
    Level0, Level1, Level2, Level3, Level4, Level5, Level6, Level7, Level8, Level9, Level10,
    Level11, Level12, Level13, Level14, Level15, Level16,
]);

// verify Level::LEVEL is set correctly
const _: [u8; 16] = [0; Level16::LEVEL];

pub struct KeyElement<'a, L: Level>(NonZeroU32, PhantomData<(&'a u8, L)>);

impl<'a, L: Level> KeyElement<'a, L> {
    pub fn
}
