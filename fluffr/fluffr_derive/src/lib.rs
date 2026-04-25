// fluffr/flatr_derive/src/lib.rs
use proc_macro::TokenStream;
mod table_macro;
mod data_macro;
mod union_macro;
mod row_macro;
mod query;
use query::*;
use table_macro::{flat_table};
use data_macro::flat;
use union_macro::flat_union;
mod helpers;
mod meta;
use helpers::*;
use meta::*;
use crate::row_macro::as_row;


#[proc_macro_derive(Flat)]
pub fn derive_flat_pod(input: TokenStream) -> TokenStream {
    flat(input)
}

#[proc_macro_derive(Table, attributes(array, table, string, scalar, union, default, file))]
pub fn derive_flat_table(input:TokenStream) -> TokenStream {
    flat_table(input)
}

#[proc_macro_derive(FlatUnion)]
pub fn derive_flat_union(input: TokenStream) -> TokenStream {
    flat_union(input)
}

#[proc_macro_derive(Row, attributes(string, table, union, array, inline, scalar, default, file, key))]
pub fn derive_as_row(input: TokenStream) -> TokenStream {
    as_row(input)
}