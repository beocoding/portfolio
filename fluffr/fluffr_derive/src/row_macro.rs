use proc_macro2::TokenStream as TokenStream2;
use syn::{DeriveInput, Ident, parse_macro_input};
use quote::quote;

use crate::*;

pub fn as_row(input: TokenStream) -> TokenStream {
    let input_di = parse_macro_input!(input as DeriveInput);

    let row_meta = analyze_meta(input_di.clone());
    validate_key(&row_meta);   // ← fails fast with a clear message

    let row_label      = &input_di.ident;
    let registry_label = row_registry(row_label);

    // ── Row trait impl (write_as_registry) ────────────────────────────────────
    // All Table impls for the Row struct come from the user's own #[derive(Table)].
    let row_trait      = impl_write_as_registry(&row_meta, &registry_label);

    // ── Registry struct: #[derive(Table)] handles all serialization impls ─────
    // Each Row field  `name: T`  (category C) becomes `#[array(C)] name: Vec<T>`.
    let registry_fields_def: TokenStream2 = row_meta.iter_fields()
        .map(|fm| {
            let name = &fm.label;
            let ty   = &fm.ty;
            let attr = category_to_array_attr(&fm.category);
            quote! { #attr pub #name: Vec<#ty>, }
        })
        .collect();
    // ── Row/Registry specialisation impls (RowRef, RegistryView, PartialEq) ──
    let row_ref_and_eq = impl_row_ref_and_eq(&row_meta);
    let query = impl_reg_view_query(&row_meta);
    quote! {
        // ── Row trait impl ────────────────────────────────────────────────────
        // (Table impls live on the user's #[derive(Table)] — we add nothing here)
        #row_trait

        // ── Registry: #[derive(Table)] handles all serialisation/view impls ──
        #[derive(::fluffr::Table, Default, Clone,)]
        pub struct #registry_label {
            #registry_fields_def
        }

        // ── Row/Registry cross-type impls ─────────────────────────────────────
        #row_ref_and_eq

        // Query
        #query
    }
    .into()
}

fn validate_key(row_meta: &TableMeta) {
    let keys: Vec<&FieldMeta> = row_meta.iter_fields()
        .filter(|f| f.key)
        .collect();

    match keys.len() {
        0 => panic!(
            "#[derive(Row)] on `{}`: exactly one field must be marked #[key]",
            row_meta.label
        ),
        1 => {
            let k = keys[0];
            if !matches!(k.category, FieldCategory::String | FieldCategory::Inline) {
                panic!(
                    "#[derive(Row)] on `{}`: key field `{}` must be a string or scalar \
                     (String, &str, or a #[repr(C)] scalar), not {:?}",
                    row_meta.label, k.label, k.category
                );
            }
        }
        _ => {
            let names: Vec<_> = keys.iter().map(|f| f.label.to_string()).collect();
            panic!(
                "#[derive(Row)] on `{}`: only one #[key] field is allowed, found: {}",
                row_meta.label,
                names.join(", ")
            );
        }
    }
}
// ── Row::write_as_registry ────────────────────────────────────────────────────

fn registry_pass1(row_meta: &TableMeta) -> TokenStream2 {
    let mut out = TokenStream2::new();
    for f in row_meta.iter_by_complexity() {
        let slot_var = f.slot_var();
        let orig     = &f.accessor;
        out.extend(quote! {
            let mut #slot_var: usize = {
                let _s: &[_] = ::std::slice::from_ref(& #orig);
                _s.write_to(buffer)
            };
        });
    }
    out
}

fn registry_pass2(row_meta: &TableMeta) -> TokenStream2 {
    // All fields are forward-offset pointers — no union slot, no inline.
    // Reverse field order: last field written first (same layout as registry).
    row_meta.iter_fields_rev().map(|f| {
        let slot_var = f.slot_var();
        quote! {
            if #slot_var != 0usize {
                *buffer.head_mut() -= 4;
                *buffer.head_mut() &= !3;
                let _h = buffer.head();
                let _jump = (buffer.slot() - #slot_var) as u32;
                buffer.buffer_mut()[_h.._h + 4].copy_from_slice(&_jump.to_le_bytes());
                #slot_var = buffer.slot();
            }
        }
    }).collect()
}

fn impl_write_as_registry(row_meta: &TableMeta, registry_label: &Ident) -> TokenStream2 {
    let row_label = &row_meta.label;

    let pass1 = registry_pass1(row_meta);
    let pass2 = registry_pass2(row_meta);

    let pass3: TokenStream2 = row_meta.iter_fields().map(|f| {
        let slot_var      = f.slot_var();
        let vtable_offset = 4 + f.offset * 2;
        quote! {
            if #slot_var != 0usize {
                let _off = (table_start_slot - #slot_var) as u16;
                vtable[#vtable_offset..#vtable_offset + 2].copy_from_slice(&_off.to_le_bytes());
            }
        }
    }).collect();

    quote! {
        impl ::fluffr::Row for #row_label {
            type Registry = #registry_label;

            #[inline(never)]
            fn write_as_registry<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                use ::fluffr::Table;
                let mut vtable = <#registry_label as ::fluffr::Table>::VTABLE_TEMPLATE;
                #pass1
                *buffer.head_mut() -= 3;
                *buffer.head_mut() &= !3;
                let table_end_slot = buffer.slot();
                #pass2
                *buffer.head_mut() -= 4;
                let table_start_slot = buffer.slot();
                let tsize = (table_start_slot - table_end_slot) as u16;
                vtable[2..4].copy_from_slice(&tsize.to_le_bytes());
                #pass3
                buffer.share_vtable(&vtable, table_start_slot);
                table_start_slot
            }
        }
    }
}

// ── RowRef + RegistryView + PartialEq ────────────────────────────────────────

fn impl_row_ref_and_eq(row_meta: &TableMeta) -> TokenStream2 {
    let row_label      = &row_meta.label;
    let view_label     = row_view(row_label);
    let reg_view_label = row_registry_view(row_label);
    let row_ref_label  = row_ref(row_label);
    let row_ref_str    = row_ref_label.to_string();

    // ── RowRef struct ─────────────────────────────────────────────────────────

    let row_ref_fields: TokenStream2 = row_meta.iter_fields().map(|f| {
        let name = &f.label;
        let ty   = &f.ty;
        quote! { pub #name: <#ty as ::fluffr::ReadAt<'a>>::ReadOutput, }
    }).collect();

    let get_row_fields: TokenStream2 = row_meta.iter_fields().map(|f| {
        let name = &f.label;
        quote! { #name: self.#name().get(i), }
    }).collect();

    let len_expr = row_meta.iter_fields().next()
        .map(|f| { let n = &f.label; quote! { self.#n().len() } })
        .unwrap_or(quote! { 0usize });

    // ── Where bounds ──────────────────────────────────────────────────────────
    // eq_where    — RowRef↔RowRef and View↔RowRef: needs ReadOutput: PartialEq
    // owned_eq_where — Owned↔RowRef: needs T: PartialEq<ReadOutput> only
    //                  (Owned↔View is generated by flat_table's impl_owned_view_eq)

    let make_bounds = |per_field: &dyn Fn(&FieldMeta) -> TokenStream2| -> TokenStream2 {
        let bounds: Vec<TokenStream2> = row_meta.iter_fields()
            .filter(|f| matches!(f.category, FieldCategory::Table | FieldCategory::Union))
            .map(per_field)
            .collect();
        if bounds.is_empty() { quote! {} } else { quote! { where #(#bounds),* } }
    };
    let eq_where    = make_bounds(&|f| { let ty = &f.ty; quote! { <#ty as ::fluffr::ReadAt<'a>>::ReadOutput: PartialEq } });
    let debug_where = make_bounds(&|f| { let ty = &f.ty; quote! { <#ty as ::fluffr::ReadAt<'a>>::ReadOutput: ::std::fmt::Debug } });
    let owned_eq_where = make_bounds(&|f| { let ty = &f.ty; quote! { #ty: PartialEq<<#ty as ::fluffr::ReadAt<'a>>::ReadOutput> } });

    // ── PartialEq field expressions ───────────────────────────────────────────

    let rowref_eq_self = join_and(row_meta.iter_fields().map(|f| {
        let name = &f.label;
        quote! { self.#name == other.#name }
    }).collect());

    let view_eq_rowref = join_and(row_meta.iter_fields().map(|f| {
        let name = &f.label;
        quote! { self.#name() == other.#name }
    }).collect());

    let owned_eq_rowref = join_and(row_meta.iter_fields().map(|f| {
        let name = &f.label;
        match &f.category {
            FieldCategory::String => quote! { self.#name.as_str() == other.#name },
            _                     => quote! { self.#name == other.#name },
        }
    }).collect());

    // ── Debug field entries ───────────────────────────────────────────────────

    let debug_fields: TokenStream2 = row_meta.iter_fields().map(|f| {
        let name     = &f.label;
        let name_str = name.to_string();
        quote! { .field(#name_str, &self.#name) }
    }).collect();


    quote! {
        // ── RowRef ────────────────────────────────────────────────────────────

        #[derive(Clone, Copy)]
        pub struct #row_ref_label<'a> {
            #row_ref_fields
        }

        impl<'a> #reg_view_label<'a> {
        }

        impl<'a> ::fluffr::RegistryView<'a> for #reg_view_label<'a> {
            type RowRef = #row_ref_label<'a>;
            #[inline]
            fn get_row(&self, i: usize) -> #row_ref_label<'a> {
                #row_ref_label { #get_row_fields }
            }
            #[inline]
            fn len(&self) -> usize { #len_expr }
        }

        // ── Debug for RowRef ──────────────────────────────────────────────────

        impl<'a> ::std::fmt::Debug for #row_ref_label<'a> #debug_where {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(#row_ref_str)
                    #debug_fields
                    .finish()
            }
        }

        // ── PartialEq: RowRef ↔ RowRef ────────────────────────────────────────

        impl<'a> PartialEq for #row_ref_label<'a> #eq_where {
            #[inline]
            fn eq(&self, other: &Self) -> bool { #rowref_eq_self }
        }

        // ── PartialEq: View ↔ RowRef ──────────────────────────────────────────

        impl<'a> PartialEq<#row_ref_label<'a>> for #view_label<'a> #eq_where {
            #[inline]
            fn eq(&self, other: &#row_ref_label<'a>) -> bool { #view_eq_rowref }
        }
        impl<'a> PartialEq<#view_label<'a>> for #row_ref_label<'a> #eq_where {
            #[inline]
            fn eq(&self, other: &#view_label<'a>) -> bool { other == self }
        }

        // ── PartialEq: Owned ↔ RowRef ─────────────────────────────────────────

        impl<'a> PartialEq<#row_ref_label<'a>> for #row_label #owned_eq_where {
            #[inline]
            fn eq(&self, other: &#row_ref_label<'a>) -> bool { #owned_eq_rowref }
        }
        impl<'a> PartialEq<#row_label> for #row_ref_label<'a> #owned_eq_where {
            #[inline]
            fn eq(&self, other: &#row_label) -> bool { other == self }
        }
    }
}


