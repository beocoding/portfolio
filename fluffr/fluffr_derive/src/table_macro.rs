use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;
use quote::{format_ident, quote};

use crate::*;

// ── Shared codegen helpers ────────────────────────────────────────────────────

/// Build a `where` clause from a possibly-empty list of bounds.
fn where_clause(bounds: Vec<TokenStream2>) -> TokenStream2 {
    if bounds.is_empty() { quote! {} } else { quote! { where #(#bounds),* } }
}

/// One bound per Table/Union field. These are the only categories whose view
/// accessors return generated types, so PartialEq/Debug impls on views need
/// explicit bounds for them.
fn indirect_bounds(
    meta:      &TableMeta,
    per_field: impl Fn(&syn::Type) -> TokenStream2,
) -> Vec<TokenStream2> {
    meta.iter_fields()
        .filter(|f| matches!(f.category, FieldCategory::Table | FieldCategory::Union))
        .map(|f| per_field(&f.ty))
        .collect()
}

/// Final vtable-hydration pass shared by owned serialize (pass 3), `merge_into`
/// (pass 3), and `vtable_bytes`: for every present field, patch its u16 vtable
/// entry with the distance from `base` to the field's recorded slot.
fn vtable_hydrate(meta: &TableMeta, base: TokenStream2) -> TokenStream2 {
    meta.iter_fields().map(|f| {
        let slot_var    = f.slot_var();
        let vtable_byte = f.vtable_byte();
        quote! {
            if #slot_var != 0usize {
                let _off = (#base - #slot_var) as u16;
                vtable[#vtable_byte..#vtable_byte + 2].copy_from_slice(&_off.to_le_bytes());
            }
        }
    }).collect()
}

// ── Serialize pass generators (shared by owned and view paths) ────────────────
//
// Both paths use the same three-pass structure; they differ only in how a
// field's presence is tested and how its value is fetched (`guard_write`), and
// in whether slot variables must be kept up to date for a later pass 3
// (`track_slots` — true for the owned path, false for the view path, which
// reuses the source vtable verbatim and has no pass 3).

type GuardWrite<'m> = &'m dyn Fn(&FieldMeta) -> (TokenStream2, TokenStream2);

fn owned_guard_write(f: &FieldMeta) -> (TokenStream2, TokenStream2) {
    let absent   = f.is_absent_check();
    let accessor = &f.accessor;
    (quote! { !#absent }, quote! { #accessor })
}

fn view_guard_write(f: &FieldMeta) -> (TokenStream2, TokenStream2) {
    let idx  = f.offset;
    let name = &f.label;
    (quote! { self.is_present(#idx) }, quote! { self.#name() })
}

/// Pass 1 — write every indirect field's payload (most complex first) and
/// record its slot in a `let mut slot_<field>` binding.
fn serialize_pass1(meta: &TableMeta, guard_write: GuardWrite) -> TokenStream2 {
    meta.iter_by_complexity()
        .filter(|f| f.category.is_indirect())
        .map(|f| {
            let slot_var       = f.slot_var();
            let (guard, write) = guard_write(f);
            quote! {
                let mut #slot_var: usize = if #guard {
                    ::fluffr::Serialize::write_to_unchecked(&#write, buffer)
                } else { 0usize };
            }
        })
        .collect()
}

/// Pass 2 — walk fields in reverse declaration order emitting the table
/// object itself: inline values directly, 5-byte slots for unions, and 4-byte
/// forward offsets for all other indirect fields.
fn serialize_pass2(meta: &TableMeta, guard_write: GuardWrite, track_slots: bool) -> TokenStream2 {
    meta.iter_fields_rev().map(|f| {
        let slot_var       = f.slot_var();
        let (guard, write) = guard_write(f);
        match &f.category {
            FieldCategory::Inline => if track_slots {
                quote! {
                    let #slot_var: usize = if #guard {
                        ::fluffr::Serialize::write_to_unchecked(&#write, buffer)
                    } else { 0usize };
                }
            } else {
                quote! {
                    if #guard { ::fluffr::Serialize::write_to_unchecked(&#write, buffer); }
                }
            },
            FieldCategory::Union => {
                let call = quote! {
                    ::fluffr::write_union_slot(buffer, #slot_var, (#write).__flat_type_id())
                };
                let stmt = if track_slots { quote! { #slot_var = #call; } } else { quote! { #call; } };
                quote! { if #slot_var != 0usize { #stmt } }
            },
            _ => {
                let record = if track_slots { quote! { #slot_var = buffer.slot(); } } else { quote! {} };
                quote! {
                    if #slot_var != 0usize {
                        *buffer.head_mut() -= 4;
                        *buffer.head_mut() &= !3;
                        let _h = buffer.head();
                        let _jump = (buffer.slot() - #slot_var) as u32;
                        // Safety: `_unchecked` contract — capacity ensured by caller.
                        debug_assert!(_h + 4 <= buffer.len());
                        unsafe {
                            ::std::ptr::copy_nonoverlapping(
                                _jump.to_le_bytes().as_ptr(),
                                buffer.buffer_mut().as_mut_ptr().add(_h),
                                4,
                            );
                        }
                        #record
                    }
                }
            },
        }
    }).collect()
}

// ── Shared Serialize scaffolding ──────────────────────────────────────────────

fn impl_serialize(
    generics:    TokenStream2,
    ty:          &TokenStream2,
    meta:        &TableMeta,
    size_tokens: TokenStream2,
    body:        TokenStream2,
    is_absent:   TokenStream2,
) -> TokenStream2 {
    let object_size = meta.vtable_size; // fields * 2 + 4 — same shape as the vtable
    quote! {
        impl #generics ::fluffr::Serialize for #ty {
            const SIZE: usize = 4;
            const ALIGN: usize = 4;
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Offset;
            #[inline]
            fn size_hint(&self) -> usize {
                let mut table_size: usize = #object_size + 8;
                #size_tokens
                table_size
            }
            #[inline]
            fn write_to<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(::fluffr::Serialize::size_hint(self));
                ::fluffr::Serialize::write_to_unchecked(self, buffer)
            }
            #[inline(never)]
            fn write_to_unchecked<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize { #body }
            #[inline(always)]
            fn is_absent(&self) -> bool { #is_absent }
        }
    }
}

// ── View PartialEq + Debug ────────────────────────────────────────────────────

/// Generated for every Table derive so that LabelView can be compared by value.
fn impl_view_eq_and_debug(meta: &TableMeta) -> TokenStream2 {
    let view_label = format_ident!("{}View", &meta.label);
    let label_str  = meta.label.to_string();

    // PartialEq: two views are equal if every field accessor returns equal values.
    let eq_where = where_clause(indirect_bounds(meta, |ty| {
        quote! { <#ty as ::fluffr::ReadAt<'a>>::ReadOutput: PartialEq }
    }));

    let eq_fields = join_and(meta.iter_fields().map(|f| {
        let name = &f.label;
        quote! { self.#name() == other.#name() }
    }).collect());

    // Debug: print struct-like representation with field names and values.
    let debug_where = where_clause(indirect_bounds(meta, |ty| {
        quote! { <#ty as ::fluffr::ReadAt<'a>>::ReadOutput: ::std::fmt::Debug }
    }));

    let debug_fields: TokenStream2 = meta.iter_fields().map(|f| {
        let name     = &f.label;
        let name_str = name.to_string();
        quote! { .field(#name_str, &self.#name()) }
    }).collect();

    quote! {
        impl<'a> PartialEq for #view_label<'a> #eq_where {
            #[inline]
            fn eq(&self, other: &Self) -> bool { #eq_fields }
        }

        impl<'a> ::std::fmt::Debug for #view_label<'a> #debug_where {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(#label_str)
                    #debug_fields
                    .finish()
            }
        }
    }
}

// ── Owned Debug ───────────────────────────────────────────────────────────────

fn impl_table_debug(meta: &TableMeta) -> TokenStream2 {
    let label     = &meta.label;
    let label_str = label.to_string();

    let where_tokens = where_clause(indirect_bounds(meta, |ty| {
        quote! { #ty: ::std::fmt::Debug }
    }));

    let debug_fields: TokenStream2 = meta.iter_fields().map(|f| {
        let name_str = f.label.to_string();
        let accessor = &f.accessor;
        quote! { .field(#name_str, &#accessor) }
    }).collect();

    quote! {
        impl ::std::fmt::Debug for #label #where_tokens {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(#label_str)
                    #debug_fields
                    .finish()
            }
        }
    }
}

fn impl_owned_field_accessors(meta: &TableMeta) -> TokenStream2 {
    let label = &meta.label;
    let methods: TokenStream2 = meta.iter_fields().map(|f| {
        let method_name = format_ident!("field_{}", f.offset);
        let accessor    = &f.accessor; // already `self.field_name` or `self.0`
        let ty          = &f.ty;
        quote! {
            #[inline(always)]
            pub const fn #method_name(&self) -> &#ty { &#accessor }
        }
    }).collect();
    quote! { impl #label { #methods } }
}

// ── Main codegen ──────────────────────────────────────────────────────────────

pub(crate) fn flat_table_from(input: DeriveInput) -> TokenStream {
    let meta = analyze_meta(input);

    let owned_ser     = impl_owned_serialize(&meta);
    let view          = impl_view(&meta);
    let view_ser      = impl_view_serialize(&meta);
    let read_at       = impl_read_at_trait(&meta);
    let verify        = impl_verify(&meta);
    let view_eq       = impl_view_eq_and_debug(&meta);
    let owned_eq      = impl_owned_self_eq(&meta);
    let table_debug   = impl_table_debug(&meta);
    let field_access  = impl_owned_field_accessors(&meta);
    let vtable_gen    = impl_vtable_gen(&meta);
    let owned_view_eq = impl_owned_view_eq(&meta);

    quote! {
        #owned_ser
        #view
        #view_ser
        #read_at
        #verify
        #view_eq
        #owned_eq
        #table_debug
        #field_access
        #vtable_gen
        #owned_view_eq
    }.into()
}

// ── Owned serialize ───────────────────────────────────────────────────────────

fn impl_owned_serialize(meta: &TableMeta) -> TokenStream2 {
    let label      = &meta.label;
    let view_label = format_ident!("{}View", label);

    let pass1 = serialize_pass1(meta, &owned_guard_write);
    let pass2 = serialize_pass2(meta, &owned_guard_write, true);
    let pass3 = vtable_hydrate(meta, quote! { table_start_slot });

    let body = quote! {
        use ::fluffr::Table;
        let mut vtable = <Self as Table>::VTABLE_TEMPLATE;
        #pass1
        let table_end_slot = buffer.slot();
        #pass2
        *buffer.head_mut() -= 4;
        let table_start_slot = buffer.slot();
        let tsize = (table_start_slot - table_end_slot) as u16;
        vtable[2..4].copy_from_slice(&tsize.to_le_bytes());
        #pass3
        buffer.share_vtable(&vtable, table_start_slot);
        table_start_slot
    };

    let size_tokens: TokenStream2 = meta.iter_fields().map(|f| {
        let a = &f.accessor;
        quote! { table_size += ::fluffr::Serialize::size_hint(&#a); }
    }).collect();

    let is_absent = join_and(meta.iter_fields().map(|f| f.is_absent_check()).collect());

    let serialize_impl = impl_serialize(quote! {}, &quote! { #label }, meta, size_tokens, body, is_absent);
    quote! {
        #serialize_impl
        impl #label {
            #[inline(always)]
            pub fn view_at_slot(buf: &[u8], slot: usize) -> #view_label<'_> {
                #view_label::new(buf, buf.len() - slot)
            }
        }
    }
}

// ── View serialize ────────────────────────────────────────────────────────────

fn impl_block_end(meta: &TableMeta) -> TokenStream2 {
    let arms: TokenStream2 = meta.iter_by_complexity().map(|f| {
        let name = &f.label;
        let ty   = &f.ty;
        match &f.category {
            FieldCategory::List(inner) => {
                let end_expr = gen_list_block_end(inner, ty);
                quote! { let list = self.#name(); if list.len > 0 { return #end_expr; } }
            }
            FieldCategory::String | FieldCategory::FileBlob => quote! {
                if self.is_present(0) {
                    let field_pos = self.t_pos + self.voff(0);
                    let abs = field_pos + u32::read_at(self.buf, field_pos) as usize;
                    return abs + 4 + u32::read_at(self.buf, abs) as usize;
                }
            },
            FieldCategory::Table => quote! {
                if self.is_present(0) { return self.#name().block_end(); }
            },
            FieldCategory::Inline => {
                let idx = f.offset;
                quote! {
                    if self.voff(#idx) != 0 {
                        return self.t_pos + self.voff(#idx) + ::std::mem::size_of::<#ty>();
                    }
                }
            }
            FieldCategory::Union => {
                let idx = f.offset;
                quote! {
                    if self.voff(#idx) != 0 {
                        return self.t_pos + self.voff(#idx) + 5;
                    }
                }
            }
        }
    }).collect();

    quote! {
        #[inline(always)]
        pub fn block_end(&self) -> usize { #arms self.t_pos }
    }
}

fn impl_view_serialize(meta: &TableMeta) -> TokenStream2 {
    let view_label = format_ident!("{}View", &meta.label);
    let pass1 = serialize_pass1(meta, &view_guard_write);
    let pass2 = serialize_pass2(meta, &view_guard_write, false);

    let body = quote! {
        #pass1
        #pass2
        *buffer.head_mut() -= 4;
        let table_start_slot = buffer.slot();
        let vt_size = u16::read_at(self.buf, self.v_pos) as usize;
        let vtable  = &self.buf[self.v_pos..self.v_pos + vt_size];
        buffer.share_vtable(vtable, table_start_slot);
        table_start_slot
    };

    let size_tokens: TokenStream2 = meta.iter_fields().map(|f| {
        let name = &f.label;
        quote! { table_size += ::fluffr::Serialize::size_hint(&self.#name()); }
    }).collect();

    impl_serialize(
        quote! { <'a> }, &quote! { #view_label<'a> },
        meta, size_tokens, body, quote! { true },
    )
}

// ── View struct + accessors ───────────────────────────────────────────────────

fn impl_view(meta: &TableMeta) -> TokenStream2 {
    let label          = &meta.label;
    let view_label     = format_ident!("{}View", label);
    let block_end_impl = impl_block_end(meta);

    let methods: TokenStream2 = meta.iter_fields().map(|f| {
        let name  = &f.label;
        let index = f.offset;
        let ty    = &f.ty;

        let body = match &f.category {
            FieldCategory::Union => quote! {
                let data_voff = self.voff(#index);
                if data_voff == 0 { return Default::default(); }
                let field_pos   = self.t_pos + data_voff;
                let tag         = u8::read_at(self.buf, field_pos + 4);
                let payload_pos = field_pos + u32::read_at(self.buf, field_pos) as usize;
                <#ty as ::fluffr::ReadAt<'a>>::read_with_tag_at(self.buf, payload_pos, tag)
            },
            FieldCategory::Inline => quote! {
                let voff = self.voff(#index);
                if voff == 0 { return Default::default(); }
                <#ty as ::fluffr::ReadAt<'a>>::read_at(self.buf, self.t_pos + voff)
            },
            _ => quote! {
                let voff = self.voff(#index);
                if voff == 0 { return Default::default(); }
                let field_pos = self.t_pos + voff;
                let abs = field_pos + u32::read_at(self.buf, field_pos) as usize;
                <#ty as ::fluffr::ReadAt<'a>>::read_at(self.buf, abs)
            },
        };

        quote! {
            #[inline(always)]
            pub fn #name(&self) -> <#ty as ::fluffr::ReadAt<'a>>::ReadOutput
            where <#ty as ::fluffr::ReadAt<'a>>::ReadOutput: Default
            { #body }
        }
    }).collect();

    let merge_method = if meta.merge { impl_merge_into(meta) } else { quote! {} };

    quote! {
        #[derive(Clone, Copy, Default)]
        pub struct #view_label<'a>(pub ::fluffr::RawView<'a>);
        impl<'a> #view_label<'a> {
            #[inline(always)]
            pub fn new(buf: &'a [u8], table_pos: usize) -> Self {
                Self(::fluffr::RawView::new(buf, table_pos))
            }
            #[inline(always)]
            pub fn from_slot(buf: &'a [u8], slot: usize) -> Self {
                Self(::fluffr::RawView::from_slot(buf, slot))
            }
            #block_end_impl
            #merge_method
            #methods
        }
        impl<'a> ::fluffr::HasRawView<'a> for #view_label<'a> {
            #[inline(always)]
            fn raw_view(&self) -> &::fluffr::RawView<'a> { &self.0 }
            #[inline(always)]
            fn block_end_dyn(&self) -> usize { self.block_end() }
        }

        impl<'a> ::std::ops::Deref for #view_label<'a> {
            type Target = ::fluffr::RawView<'a>;
            #[inline(always)]
            fn deref(&self) -> &Self::Target { &self.0 }
        }
    }
}

// ── merge_into (generated only when every field is a list) ────────────────────

fn impl_merge_into(meta: &TableMeta) -> TokenStream2 {
    let table_label = &meta.label;

    let merge_pass1: TokenStream2 = meta.iter_by_complexity().map(|f| {
        let slot_var = f.slot_var();
        let name     = &f.label;
        match &f.category {
            FieldCategory::List(inner) if matches!(inner.as_ref(), FieldCategory::Inline) => {
                let elem_ty = vec_inner(&f.ty).expect("List(Inline) must be Vec<T>");
                quote! {
                    let mut #slot_var = ::fluffr::merge_inline_list(
                        &_views, out,
                        ::std::mem::size_of::<#elem_ty>(),
                        ::std::mem::align_of::<#elem_ty>().max(4) - 1,
                        |_v| _v.#name(),
                    );
                }
            },
            FieldCategory::List(inner) if matches!(inner.as_ref(), FieldCategory::String) => {
                quote! { let mut #slot_var = ::fluffr::merge_string_list(&_views, out, |_v| _v.#name()); }
            },
            FieldCategory::List(inner) if matches!(inner.as_ref(), FieldCategory::FileBlob) => {
                let ty       = &f.ty;
                let inner_ty = vec_inner(ty).unwrap_or(ty);
                quote! {
                    let mut #slot_var = ::fluffr::merge_file_list(
                        &_views, out,
                        |_v| -> ::fluffr::ListView<'_, #inner_ty> { _v.#name() }
                    );
                }
            },
            FieldCategory::List(inner) if matches!(inner.as_ref(), FieldCategory::Table) => {
                quote! {
                    let mut #slot_var = ::fluffr::merge_table_list(
                        &_views, out, |_v| _v.#name(), |_el| _el.block_end(),
                    );
                }
            },
            FieldCategory::List(inner) if matches!(inner.as_ref(), FieldCategory::Union) => {
                quote! { let mut #slot_var = ::fluffr::merge_union_list(&_views, out, |_v| _v.#name()); }
            },
            _ => quote! {
                let mut _temp: Vec<usize> = Vec::new();
                for view in _views.iter() {
                    for elem in view.#name().rev() {
                        _temp.push(::fluffr::Serialize::write_to_unchecked(&elem, out));
                    }
                }
                let _len = _temp.len() as u32;
                for target_slot in _temp {
                    *out.head_mut() -= 4;
                    *out.head_mut() &= !3;
                    let _head = out.head();
                    let _jump = (out.slot() - target_slot) as u32;
                    // Safety: capacity ensured by merge preamble.
                    debug_assert!(_head + 4 <= out.len());
                    unsafe {
                        ::std::ptr::copy_nonoverlapping(
                            _jump.to_le_bytes().as_ptr(),
                            out.buffer_mut().as_mut_ptr().add(_head),
                            4,
                        );
                    }
                }
                let mut #slot_var = if _len == 0 { 0usize } else { _len.write_to_unchecked(out) };
            },
        }
    }).collect();

    let merge_pass2: TokenStream2 = meta.iter_fields_rev().map(|f| {
        let slot_var = f.slot_var();
        quote! {
            if #slot_var != 0usize {
                let _jump = (out.slot() + 4 - #slot_var) as u32;
                #slot_var = _jump.write_to_unchecked(out);
            }
        }
    }).collect();

    let merge_pass3 = vtable_hydrate(meta, quote! { table_start_slot });

    quote! {
        #[inline(never)]
        pub fn merge_into<B: ::fluffr::Buffer>(
            &self, buffer: &'a [u8], slots: &[usize], out: &mut B,
        ) {
            out.reset();
            // Small-count fast path: views are Copy, so up to 16 sources are
            // assembled on the stack, avoiding a per-merge heap allocation.
            let _n = slots.len() + 1;
            let mut _stack: [Self; 16] = [Self::default(); 16];
            let mut _heap: Vec<Self>;
            let _views: &[Self] = if _n <= 16 {
                for (_i, &_slot) in slots.iter().enumerate() {
                    _stack[_i] = Self::from_slot(buffer, _slot);
                }
                _stack[slots.len()] = *self;
                &_stack[.._n]
            } else {
                _heap = slots.iter()
                    .map(|&slot| Self::from_slot(buffer, slot))
                    .chain(::std::iter::once(*self))
                    .collect();
                &_heap
            };
            let mut vtable = <#table_label as ::fluffr::Table>::VTABLE_TEMPLATE;
            #merge_pass1
            let table_end_slot = out.slot();
            #merge_pass2
            *out.head_mut() -= 4;
            let table_start_slot = out.slot();
            let tsize = (table_start_slot - table_end_slot) as u16;
            vtable[2..4].copy_from_slice(&tsize.to_le_bytes());
            #merge_pass3
            out.share_vtable(&vtable, table_start_slot);
            out.finish(table_start_slot);
        }
    }
}

// ── Table + ReadAt impls ──────────────────────────────────────────────────────

fn impl_read_at_trait(meta: &TableMeta) -> TokenStream2 {
    let label       = &meta.label;
    let view_label  = format_ident!("{}View", label);
    let vtable_size = meta.vtable_size;
    let vtable_bytes = {
        let mut v = vec![0u8; vtable_size];
        let s = (vtable_size as u16).to_le_bytes();
        v[0] = s[0]; v[1] = s[1];
        v
    };
    let vtable_bytes = vtable_bytes.iter();
    quote! {
        impl ::fluffr::Table for #label {
            type VTableTemplate = [u8; #vtable_size];
            const VTABLE_TEMPLATE: Self::VTableTemplate = [#(#vtable_bytes),*];
            type View<'a> = #view_label<'a>;
            #[inline(always)]
            fn view<'a>(buf: &'a [u8], table_idx: usize) -> #view_label<'a> {
                #view_label::new(buf, table_idx)
            }
            #[inline]
            fn as_buffer(&self) -> ::fluffr::DefaultBuffer {
                let mut buffer = ::fluffr::DefaultBuffer::new(::fluffr::Serialize::size_hint(self));
                let slot = ::fluffr::Serialize::write_to_unchecked(self, &mut buffer);
                buffer.finish(slot);
                buffer
            }
        }
        impl<'a> ::fluffr::ReadAt<'a> for #label {
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Offset;
            type ReadOutput = #view_label<'a>;
            #[inline(always)]
            fn read_at(buf: &'a [u8], offset: usize) -> #view_label<'a> { Self::view(buf, offset) }
            #[inline(always)]
            fn default_output() -> #view_label<'a> { #view_label::default() }
            #[inline(always)]
            fn payload_block_end(buf: &'a [u8], pos: usize) -> usize {
                Self::view(buf, pos).block_end()
            }
        }
    }
}

// ── Verify ────────────────────────────────────────────────────────────────────

fn emit_field_verify(f: &FieldMeta) -> TokenStream2 {
    let ty = &f.ty;
    match &f.category {
        FieldCategory::Inline => quote! {},
        FieldCategory::String => quote! { ::fluffr::verify_string_field(buf, field_pos)?; },
        FieldCategory::FileBlob => quote! { ::fluffr::verify_file_field(buf, field_pos)?; },
        FieldCategory::Table  => quote! {
            ::fluffr::verify_table_field::<#ty>(buf, field_pos, depth - 1, out)?;
        },
        FieldCategory::Union  => quote! {
            ::fluffr::check_bounds(buf, field_pos, 5, "union field")?;
            let _union_jump  = u32::read_at(buf, field_pos) as usize;
            let _union_tag   = u8::read_at(buf, field_pos + 4);
            let _payload_pos = field_pos.saturating_add(_union_jump);
            #ty::verify_tag(_union_tag, buf, _payload_pos, depth - 1, out)?;
        },
        FieldCategory::List(inner) => match inner.as_ref() {
            FieldCategory::Inline => {
                let elem_size = vec_inner(ty)
                    .map(|it| quote! { ::std::mem::size_of::<#it>() })
                    .unwrap_or(quote! { 1usize });
                quote! { ::fluffr::verify_scalar_array(buf, field_pos, #elem_size)?; }
            }
            FieldCategory::String => quote! { ::fluffr::verify_string_array(buf, field_pos)?; },
            FieldCategory::FileBlob => quote! { ::fluffr::verify_file_field(buf, field_pos)?; },
            FieldCategory::Table  => {
                let inner_ty = vec_inner(ty).expect("#[array(table)] must be Vec<T>");
                quote! { ::fluffr::verify_table_array::<#inner_ty>(buf, field_pos, depth - 1, out)?; }
            }
            FieldCategory::List(_) => quote! { ::fluffr::verify_scalar_array(buf, field_pos, 4)?; },
            FieldCategory::Union   => {
                let inner_ty = vec_inner(ty).expect("List(Union) must be Vec<T>");
                quote! {
                    ::fluffr::check_bounds(buf, field_pos, 4, "union-array forward-offset")?;
                    let _hdr       = field_pos.saturating_add(u32::read_at(buf, field_pos) as usize);
                    ::fluffr::check_bounds(buf, _hdr, 4, "union-array length prefix")?;
                    let _n         = u32::read_at(buf, _hdr) as usize;
                    let _jump_base = _hdr + 4;
                    let _tag_base  = _jump_base + 4 * _n;
                    ::fluffr::check_bounds(buf, _jump_base, 4 * _n, "union-array jump table")?;
                    ::fluffr::check_bounds(buf, _tag_base,  _n,     "union-array tag section")?;
                    for _i in 0.._n {
                        let _jp  = _jump_base + _i * 4;
                        let _jv  = u32::read_at(buf, _jp);
                        let _tag = u8::read_at(buf, _tag_base + _i);
                        if _tag != 0 {
                            let _pp = _jp + _jv as usize;
                            #inner_ty::verify_tag(_tag, buf, _pp, depth - 1, out)?;
                        }
                    }
                }
            },
        },
    }
}

fn impl_verify(meta: &TableMeta) -> TokenStream2 {
    let label = &meta.label;
    let per_field: TokenStream2 = meta.iter_fields().map(|f| {
        let field_idx  = f.offset;
        let ty         = &f.ty;
        let field_size = match &f.category {
            FieldCategory::Inline => quote! { ::std::mem::size_of::<#ty>() },
            FieldCategory::Union  => quote! { 5usize },
            _                     => quote! { 4usize },
        };
        let deeper = emit_field_verify(f);
        quote! {
            if let Some(field_pos) = ::fluffr::verify_vtable_field(
                buf, v_pos, vtable_size, t_pos, object_size, #field_idx, #field_size,
            )? { #deeper }
        }
    }).collect();

    quote! {
        impl ::fluffr::Verify for #label {
            const INLINE_SIZE: usize = 4;
            #[inline]
            fn verify_at(buf: &[u8], t_pos: usize, depth: usize, out: &mut ::std::vec::Vec<usize>)
                -> ::fluffr::VerifyResult
            {
                let (v_pos, vtable_size, object_size) =
                    ::fluffr::verify_table_header(buf, t_pos, depth)?;
                out.push(v_pos);
                #per_field
                Ok(())
            }
        }
    }
}

// ── Owned ↔ View PartialEq ────────────────────────────────────────────────────

fn impl_owned_view_eq(meta: &TableMeta) -> TokenStream2 {
    let label      = &meta.label;
    let view_label = format_ident!("{}View", label);

    let where_tokens = where_clause(indirect_bounds(meta, |ty| {
        quote! { #ty: PartialEq<<#ty as ::fluffr::ReadAt<'a>>::ReadOutput> }
    }));

    let eq_fields = join_and(meta.iter_fields().map(|f| {
        let accessor = &f.accessor; // self or self.field_name
        let name     = &f.label;    // for the view accessor call
        quote! { #accessor == other.#name() }
    }).collect());

    quote! {
        impl<'a> PartialEq<#view_label<'a>> for #label #where_tokens {
            #[inline]
            fn eq(&self, other: &#view_label<'a>) -> bool { #eq_fields }
        }
        impl<'a> PartialEq<#label> for #view_label<'a> #where_tokens {
            #[inline]
            fn eq(&self, other: &#label) -> bool { other == self }
        }
    }
}

// ── Owned ↔ Owned PartialEq ───────────────────────────────────────────────────

fn impl_owned_self_eq(meta: &TableMeta) -> TokenStream2 {
    let label = &meta.label;

    let where_tokens = where_clause(indirect_bounds(meta, |ty| {
        quote! { #ty: PartialEq }
    }));

    let eq_fields = join_and(meta.iter_fields().map(|f| {
        let m = format_ident!("field_{}", f.offset);
        quote! { self.#m() == other.#m() }
    }).collect());

    quote! {
        impl PartialEq for #label #where_tokens {
            #[inline]
            fn eq(&self, other: &Self) -> bool { #eq_fields }
        }
    }
}

// ── List block-end helper ─────────────────────────────────────────────────────

fn gen_list_block_end(inner: &FieldCategory, ty: &syn::Type) -> TokenStream2 {
    match inner {
        FieldCategory::Inline => {
            let elem_ty = vec_inner(ty).expect("List(Inline) must be Vec<T>");
            quote! { list.offset + list.len * ::std::mem::size_of::<#elem_ty>() }
        }
        FieldCategory::String | FieldCategory::FileBlob => quote! {{
            let _last = list.abs_pos(list.len - 1);
            _last + 4 + u32::read_at(list.buf, _last) as usize
        }},
        FieldCategory::Table => quote! {{ list.read_last().block_end() }},
        FieldCategory::List(inner2) => {
            let inner_end = gen_list_block_end(inner2, ty);
            quote! {{ let list = list.read_last(); #inner_end }}
        }
        FieldCategory::Union => {
            let inner_ty = vec_inner(ty).expect("List(Union) must be Vec<T>");
            quote! {{
                let _last     = list.len() - 1;
                let _last_pos = list.abs_pos(_last);
                let _last_tag = u8::read_at(list.buf, list.offset + 4 * list.len() + _last);
                <#inner_ty>::__block_end_at(list.buf, _last_pos, _last_tag)
            }}
        },
    }
}

// ── VTable precomputation ─────────────────────────────────────────────────────

/// `vtable_bytes()` — compute this owned value's vtable (and object size)
/// without serializing, by replaying pass 2's reverse walk arithmetically.
fn impl_vtable_gen(meta: &TableMeta) -> TokenStream2 {
    let label       = &meta.label;
    let vtable_size = meta.vtable_size;

    let reverse_steps: TokenStream2 = meta.iter_fields_rev().map(|f| {
        let ty        = &f.ty;
        let is_absent = f.is_absent_check();
        let slot_var  = f.slot_var();
        quote! {
            let #slot_var: usize = if !(#is_absent) {
                end_offset += <#ty as ::fluffr::Serialize>::SIZE;
                // snap down to alignment, same as head &= !mask after writing
                end_offset = (end_offset + <#ty as ::fluffr::Serialize>::ALIGNR)
                            & <#ty as ::fluffr::Serialize>::ALIGN_MASK;
                end_offset
            } else {
                0
            };
        }
    }).collect();

    let forward_hydrate = vtable_hydrate(meta, quote! { object_size });

    quote! {
        impl #label {
            #[inline]
            pub fn vtable_bytes(&self) -> ([u8; #vtable_size], u16) {
                use ::fluffr::Table;
                let mut vtable = <Self as ::fluffr::Table>::VTABLE_TEMPLATE;
                let mut end_offset = 0;

                #reverse_steps

                // Align to 4, then add 4 for the vtable jump — mirrors
                // `head -= 4; head &= !3` at the end of pass2.
                let object_size = ((end_offset + 3) & !3) + 4;
                vtable[2..4].copy_from_slice(&(object_size as u16).to_le_bytes());

                #forward_hydrate

                (vtable, object_size as u16)
            }
        }
    }
}