// fluffr/flatr_derive/src/lib.rs
use proc_macro::TokenStream;
mod table_macro;
mod data_macro;
mod union_macro;
mod meta;
use table_macro::flat_table;
use data_macro::flat;
use union_macro::flat_union;
use meta::*;

#[proc_macro_derive(Flat)]
pub fn derive_flat_pod(input: TokenStream) -> TokenStream {
    flat(input)
}

#[proc_macro_derive(Table, attributes(array, table, string, scalar, union, default, file))]
pub fn derive_flat_table(input: TokenStream) -> TokenStream {
    flat_table(input)
}

#[proc_macro_derive(FlatUnion)]
pub fn derive_flat_union(input: TokenStream) -> TokenStream {
    flat_union(input)
}