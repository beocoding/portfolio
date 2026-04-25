use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::*;
pub fn impl_reg_view_query(row_meta: &TableMeta) -> TokenStream2 {
    let row_label    = &row_meta.label;
    let row_reg_view = row_registry_view(row_label);
    let query_type   = row_query_type(row_label);

    // ── Key field ─────────────────────────────────────────────────────────────
    let key_field = row_meta.iter_fields()
        .find(|f| f.key)
        .expect("impl_reg_view_query called on a Row with no #[key] field");
    let key_ty    = &key_field.ty;
    let key_label = &key_field.label;

    // ── Query struct fields ───────────────────────────────────────────────────
    let query_fields: TokenStream2 = row_meta.iter_fields()
        .map(|f| {
            let label = &f.label;
            let ty    = &f.ty;
            quote! { pub #label: ::core::option::Option<#ty>, }
        })
        .collect();

    let default_fields: TokenStream2 = row_meta.iter_fields()
        .map(|f| {
            let label = &f.label;
            quote! { #label: ::core::option::Option::None, }
        })
        .collect();

    let builder_methods: TokenStream2 = row_meta.iter_fields()
        .map(|f| {
            let label = &f.label;
            let ty    = &f.ty;
            quote! {
                #[inline]
                pub fn #label(mut self, val: #ty) -> Self {
                    self.#label = ::core::option::Option::Some(val);
                    self
                }
            }
        })
        .collect();

    // ── query() body ──────────────────────────────────────────────────────────
    let first_field  = row_meta.iter_by_simplicity().next().unwrap();
    let sample_field = { let l = &first_field.label; quote! { self.#l() } };

    let field_arms: TokenStream2 = row_meta.iter_by_simplicity()
        .map(|f| {
            let accessor    = &f.accessor;
            let field_label = &f.label;
            quote! {
                if !mask.is_full() {
                    if let Some(ref val) = query.#field_label {
                        let misses = mask.clone();
                        let mut view = #accessor().with_skip(&misses);
                        while let Some(e) = view.next() {
                            let idx = view.next - 1;
                            if &e != val {
                                mask.set(idx);
                            }
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        #[doc = "Query builder for the row view."]
        #[derive(Debug, Clone, PartialEq)]
        pub struct #query_type {
            #query_fields
        }

        impl #query_type {
            #[inline(always)]
            pub const fn default() -> Self {
                Self { #default_fields }
            }
            #builder_methods
        }

        impl ::fluffr::QueryType for #query_type {
            #[inline(always)]
            fn new() -> Self { Self::default() }
        }

        impl<'a> ::fluffr::Query<'a> for #row_reg_view<'a> {
            type QueryType = #query_type;
            type Key       = #key_ty;

            #[inline(always)]
            fn query(&self, query: #query_type) -> ::fluffr::BitMask {
                let len = #sample_field.len();
                let mut mask = ::fluffr::BitMask::new(len);
                #field_arms
                !mask
            }

            #[inline]
            fn query_by_key(
                &self,
                val: <#key_ty as ::fluffr::ReadAt<'a>>::ReadOutput,
            ) -> ::core::option::Option<usize> {
                let view = self.#key_label();
                for i in 0..view.len() {
                    if view.get(i) == val {
                        return ::core::option::Option::Some(i);
                    }
                }
                ::core::option::Option::None
            }

            #[inline]
            fn query_by_keys(
                &self,
                vals: &[<#key_ty as ::fluffr::ReadAt<'a>>::ReadOutput],
            ) -> ::fluffr::BitMask {
                let view = self.#key_label();
                let mut mask = ::fluffr::BitMask::new(view.len());
                for i in 0..view.len() {
                    if vals.contains(&view.get(i)) {
                        mask.set(i);
                    }
                }
                mask
            }
        }
    }
}