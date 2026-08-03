// fluffr/flatr_derive/src/lib.rs
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput, Fields};
mod table_macro;
mod data_macro;
mod union_macro;
mod row_macro;
mod query;
use query::*;
mod helpers;
mod meta;
use helpers::*;
use meta::*;
use crate::row_macro::as_row;


/// Single entry point for what used to be three derives (`Flat`, `Table`,
/// `FlatUnion`). The macro inspects the item's shape and dispatches to the
/// matching codegen path — the underlying wire formats are unchanged:
///
/// - struct, `#[repr(C)]`         → inline POD (old `#[derive(Flat)]`)
/// - struct, no `#[repr(C)]`      → offset/vtable table (old `#[derive(Table)]`)
/// - enum, all unit variants      → inline POD discriminant (old `#[derive(Flat)]`)
/// - enum, any variant has a payload → tagged union (old `#[derive(FlatUnion)]`)
#[proc_macro_derive(Flat, attributes(array, table, string, scalar, union, default, file))]
pub fn derive_flat(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match &input.data {
        Data::Struct(_) if has_repr_c(&input.attrs)   => data_macro::flat_struct(input),
        Data::Struct(_)                               => table_macro::flat_table_from(input),
        Data::Enum(e) if is_tagged_union(&e.variants)  => union_macro::flat_union_from(input),
        Data::Enum(_)                                  => data_macro::flat_enum(input),
        Data::Union(_) => panic!("#[derive(Flat)] cannot be derived for Rust unions"),
    }
}

#[proc_macro_derive(Row, attributes(string, table, union, array, inline, scalar, default, file, key))]
pub fn derive_as_row(input: TokenStream) -> TokenStream {
    as_row(input)
}

/// A struct derives as an inline POD (raw bytes, no vtable) when marked
/// `#[repr(C)]`; otherwise it derives as an offset/vtable table.
fn has_repr_c(attrs: &[syn::Attribute]) -> bool {
    let mut found = false;
    for attr in attrs {
        if !attr.path().is_ident("repr") { continue; }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("C") { found = true; }
            Ok(())
        });
    }
    found
}

/// An enum derives as a tagged union when any variant carries a payload; an
/// all-unit-variant enum derives as an inline POD discriminant instead.
fn is_tagged_union(variants: &syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>) -> bool {
    variants.iter().any(|v| !matches!(v.fields, Fields::Unit))
}