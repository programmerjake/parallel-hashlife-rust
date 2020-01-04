macro_rules! impl_everything {
    ($mod_name:ident) => {
        #[path = "maybe_sync"]
        pub mod $mod_name {
            mod $mod_name;
            pub use $mod_name::*;

            mod generic;
            pub use generic::*;

            pub use crate::common::*;
        }
    };
}

pub mod common;

impl_everything!(sync);
impl_everything!(unsync);
