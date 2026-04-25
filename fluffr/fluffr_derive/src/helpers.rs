use proc_macro2::Ident;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};


// ── Misc helpers ──────────────────────────────────────────────────────────────

pub fn join_and(items: Vec<TokenStream2>) -> TokenStream2 {
    items.into_iter()
        .reduce(|a, b| quote! { #a && #b })
        .unwrap_or(quote! { true })
}

pub fn row_view(row_label: &Ident)-> Ident {
    format_ident!("{}View", row_label)
}

pub fn row_query_type(row_label: &Ident)-> Ident {
    format_ident!("{}Query", row_label)
}
pub fn row_query_builder(row_label: &Ident)-> Ident {
    format_ident!("{}QueryBuilder", row_label)
}
pub fn row_registry(row_label: &Ident)-> Ident {
    format_ident!("{}Registry", row_label)
}
pub fn row_ref(row_label: &Ident) -> Ident {
    format_ident!("{}RowRef", row_label)

}
pub fn row_registry_view(row_label: &Ident) -> Ident {
    format_ident!("{}RegistryView", row_label)

}