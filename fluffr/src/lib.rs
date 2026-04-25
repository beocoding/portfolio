// inventory/fluffr/src/lib.rs
extern crate self as fluffr;

// Re-export everything from core at the root level
pub use fluffr_core::{
    Table, 
    buffer::{*},
    serialize::{*},
    bitmax,
    Flat,
    DataType,
    read::*,
    verify::*,
    merge::*,
    Row,
    RegistryView,
    Query,
    QueryType,
    filter::*,
    file_kind::*,
};

// Re-export the derive macro
pub use fluffr_derive::{Row,Table, FlatUnion, Flat};

#[doc(hidden)]
pub mod __private {
    pub use core::*;
}

// inventory/derive_table/src/lib.rs
pub mod prelude {
    pub use fluffr_core::{
        Table, 
        buffer::{*},
        serialize::{*},
        bitmax,
        Flat,
        DataType,
        read::*,
        verify::*,
        merge::*,
        Row,
        RegistryView,
        Query,
        QueryType,
        filter::*,
        file_kind::*,
    };
    pub use fluffr_derive::{Row,Table, FlatUnion, Flat};
}
