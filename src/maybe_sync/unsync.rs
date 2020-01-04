pub trait FakeSend {}
pub trait FakeSync {}

impl<T: ?Sized> FakeSend for T {}
impl<T: ?Sized> FakeSync for T {}

pub use self::FakeSend as MaybeSend;
pub use self::FakeSync as MaybeSync;
