// fluffr/flatr_derive/src/union.rs
use proc_macro::TokenStream;
use quote::format_ident;
use syn::{Data, DeriveInput, Fields, parse_macro_input};
use quote::quote;


pub fn flat_union(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    validate(&input);
    emit(&input)
}


// ── Validation ────────────────────────────────────────────────────────────────


fn validate(input: &DeriveInput) {
    validate_repr_u8(input);
    validate_is_enum(input);
    validate_discriminants(input, enum_variants(input));
}


fn validate_repr_u8(input: &DeriveInput) {
    let ok = input.attrs.iter().any(|a| {
        a.path().is_ident("repr")
            && a.parse_args::<syn::Ident>().map_or(false, |i| i == "u8")
    });
    if !ok { panic!("#[derive(FlatUnion)] requires #[repr(u8)] on `{}`", input.ident); }
}


fn validate_is_enum(input: &DeriveInput) {
    if !matches!(input.data, Data::Enum(_)) {
        panic!("#[derive(FlatUnion)] is only valid on enums");
    }
}


fn validate_discriminants(
    input:    &DeriveInput,
    variants: &syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>,
) {
    let enum_name  = &input.ident;
    let mut next_disc: u8 = 0;
    let mut found_none    = false;

    for v in variants {
        let vname = &v.ident;
        let disc  = resolve_discriminant(enum_name, vname, &v.discriminant, next_disc);

        if disc == 0 {
            if !matches!(v.fields, Fields::Unit) {
                panic!(
                    "`{enum_name}::{vname}` has discriminant 0 but is not a unit variant. \
                     The None sentinel must be a unit variant with no payload."
                );
            }
            found_none = true;
        } else if disc < next_disc {
            panic!(
                "`{enum_name}::{vname}` discriminant {disc} collides with or precedes a \
                 previously assigned discriminant. Discriminants must be strictly increasing."
            );
        }

        next_disc = disc.checked_add(1)
            .unwrap_or_else(|| panic!("`{enum_name}::{vname}` discriminant overflows u8"));
    }

    if !found_none {
        panic!("`{enum_name}` must have a unit variant with discriminant 0, e.g. `None = 0`");
    }
}


// ── Emission ──────────────────────────────────────────────────────────────────


pub fn emit(input: &DeriveInput) -> TokenStream {
    let enum_name      = &input.ident;
    let view_enum_name = format_ident!("{}View", enum_name);
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let variants       = enum_variants(input);

    let impl_generics_no_lt = if input.generics.params.is_empty() {
        quote! {}
    } else {
        let params = &input.generics.params;
        quote! { #params, }
    };

    // Find the discriminant-0 variant for Default::default() and is_absent.
    let none_variant: syn::Ident = {
        let mut found       = None;
        let mut disc_ctr: u8 = 0;
        for v in variants.iter() {
            let d = resolve_discriminant(enum_name, &v.ident, &v.discriminant, disc_ctr);
            if d == 0 { found = Some(v.ident.clone()); break; }
            disc_ctr = d.wrapping_add(1);
        }
        found.expect("validation guarantees a discriminant-0 variant")
    };

    let mut next_disc:           u8   = 0;
    let mut view_variants             = Vec::new();
    let mut owned_tag_arms            = Vec::new();
    let mut view_tag_arms             = Vec::new();
    let mut owned_size_hint_arms      = Vec::new();
    let mut owned_write_arms          = Vec::new();
    let mut view_size_hint_arms       = Vec::new();
    let mut view_write_arms           = Vec::new();
    let mut verify_arms               = Vec::new();
    let mut read_with_tag_arms        = Vec::new();
    let mut block_end_at_arms         = Vec::new();
    let mut owned_self_eq_arms        = Vec::new();
    let mut view_eq_arms              = Vec::new();  // NEW
    let mut owned_eq_bounds           = Vec::new();

    for variant in variants {
        let vname = &variant.ident;
        let disc  = resolve_discriminant(enum_name, vname, &variant.discriminant, next_disc);
        next_disc = disc.wrapping_add(1);

        match &variant.fields {
            // ── Payload variant ───────────────────────────────────────────────
            Fields::Unnamed(f) if f.unnamed.len() == 1 => {
                let inner_ty = &f.unnamed[0].ty;

                view_variants.push(quote! {
                    #vname(<#inner_ty as ::fluffr::ReadAt<'a>>::ReadOutput),
                });

                owned_tag_arms.push(quote!       { Self::#vname(_) => #disc, });
                view_tag_arms.push(quote!        { Self::#vname(_) => #disc, });

                owned_size_hint_arms.push(quote! {
                    Self::#vname(inner) => ::fluffr::Serialize::size_hint(inner),
                });
                owned_write_arms.push(quote! {
                    Self::#vname(inner) => ::fluffr::Serialize::write_to_unchecked(inner, buffer),
                });

                view_size_hint_arms.push(quote! {
                    Self::#vname(inner) => ::fluffr::Serialize::size_hint(inner),
                });
                view_write_arms.push(quote! {
                    Self::#vname(inner) => ::fluffr::Serialize::write_to_unchecked(inner, buffer),
                });

                verify_arms.push(quote! {
                    #disc => <#inner_ty as ::fluffr::Verify>::verify_at(
                        buf, payload_pos, depth - 1, out
                    ),
                });

                read_with_tag_arms.push(quote! {
                    #disc => #view_enum_name::#vname(
                        <#inner_ty as ::fluffr::ReadAt<'a>>::read_at(buf, payload_pos)
                    ),
                });

                block_end_at_arms.push(quote! {
                    #disc => <#inner_ty as ::fluffr::ReadAt<'_>>::payload_block_end(
                        buf, payload_pos
                    ),
                });

                owned_self_eq_arms.push(quote! {
                    (Self::#vname(a), Self::#vname(b)) => a == b,
                });
                // owned ↔ view: compare payload directly
                view_eq_arms.push(quote! {
                    (Self::#vname(a), #view_enum_name::#vname(b)) => a == b,
                });
                owned_eq_bounds.push(quote! { #inner_ty: PartialEq });
            }

            // ── Unit variant (includes the discriminant-0 None sentinel) ──────
            Fields::Unit => {
                view_variants.push(quote! { #vname, });

                owned_tag_arms.push(quote!       { Self::#vname => #disc, });
                view_tag_arms.push(quote!        { Self::#vname => #disc, });
                owned_size_hint_arms.push(quote! { Self::#vname => 0, });
                owned_write_arms.push(quote!     { Self::#vname => 0, });
                view_size_hint_arms.push(quote!  { Self::#vname => 0, });
                view_write_arms.push(quote!      { Self::#vname => 0, });
                verify_arms.push(quote!          { #disc => Ok(()), });
                read_with_tag_arms.push(quote!   { #disc => #view_enum_name::#vname, });
                block_end_at_arms.push(quote!    { #disc => payload_pos, });
                owned_self_eq_arms.push(quote!   { (Self::#vname, Self::#vname) => true, });
                view_eq_arms.push(quote!         { (Self::#vname, #view_enum_name::#vname) => true, });
            }

            _ => panic!(
                "`{enum_name}::{vname}` must be a single-field tuple variant or a unit variant"
            ),
        }
    }

    let owned_eq_where = if owned_eq_bounds.is_empty() {
        quote! { #where_clause }
    } else if where_clause.is_some() {
        quote! { #where_clause #(#owned_eq_bounds,)* }
    } else {
        quote! { where #(#owned_eq_bounds),* }
    };

    TokenStream::from(quote! {

        // ── View enum ─────────────────────────────────────────────────────────

        #[derive(Clone, Copy, PartialEq, Debug)]
        pub enum #view_enum_name<'a> {
            #(#view_variants)*
        }

        impl<'a> Default for #view_enum_name<'a> {
            #[inline(always)]
            fn default() -> Self { Self::#none_variant }
        }

        impl<'a> #view_enum_name<'a> {
            #[inline(always)]
            pub const fn __flat_type_id(&self) -> u8 {
                match self { #(#view_tag_arms)* }
            }
        }

        impl<'a, #impl_generics_no_lt> ::fluffr::Serialize
            for #view_enum_name<'a> #where_clause
        {
            const SIZE: usize = 5;
            const ALIGN: usize = 4;
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Union;

            #[inline(always)]
            fn size_hint(&self) -> usize {
                match self { #(#view_size_hint_arms)* }
            }

            #[inline(always)]
            fn write_to<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(self.size_hint() + 3);
                self.write_to_unchecked(buffer)
            }

            #[inline(always)]
            fn write_to_unchecked<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                match self { #(#view_write_arms)* }
            }

            #[inline(always)]
            fn is_absent(&self) -> bool { matches!(self, Self::#none_variant) }

            #[inline(always)]
            fn tag(&self) -> u8 { self.__flat_type_id() }
        }

        // ── Owned enum inherent methods ───────────────────────────────────────

        impl<#impl_generics_no_lt> #enum_name #ty_generics #where_clause {
            #[inline(always)]
            pub fn __flat_type_id(&self) -> u8 {
                match self { #(#owned_tag_arms)* }
            }

            #[inline]
            pub fn __block_end_at(buf: &[u8], payload_pos: usize, tag: u8) -> usize {
                match tag {
                    #(#block_end_at_arms)*
                    _ => payload_pos,
                }
            }

            #[inline]
            pub fn verify_tag(
                tag:         u8,
                buf:         &[u8],
                payload_pos: usize,
                depth:       usize,
                out:         &mut ::std::vec::Vec<usize>,
            ) -> ::fluffr::VerifyResult {
                if depth == 0 {
                    return Err(::fluffr::VerifyError::DepthLimitExceeded);
                }
                match tag {
                    #(#verify_arms)*
                    _ => Err(::fluffr::VerifyError::BadOffset { at: payload_pos }),
                }
            }
        }

        // ── Serialize (owned) ─────────────────────────────────────────────────

        impl<#impl_generics_no_lt> ::fluffr::Serialize
            for #enum_name #ty_generics #where_clause
        {
            const SIZE: usize = 5;
            const ALIGN: usize = 4;
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Union;

            #[inline(always)]
            fn size_hint(&self) -> usize {
                match self { #(#owned_size_hint_arms)* }
            }

            #[inline(always)]
            fn write_to<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(self.size_hint() + 3);
                self.write_to_unchecked(buffer)
            }

            #[inline(always)]
            fn write_to_unchecked<B: ::fluffr::Buffer>(&self, buffer: &mut B) -> usize {
                match self { #(#owned_write_arms)* }
            }

            #[inline(always)]
            fn is_absent(&self) -> bool { matches!(self, Self::#none_variant) }

            #[inline(always)]
            fn tag(&self) -> u8 { self.__flat_type_id() }
        }

        // ── Verify ────────────────────────────────────────────────────────────

        impl<#impl_generics_no_lt> ::fluffr::Verify
            for #enum_name #ty_generics #where_clause
        {
            const INLINE_SIZE: usize = 5;

            #[inline]
            fn verify_at(
                buf:    &[u8],
                offset: usize,
                depth:  usize,
                out:    &mut ::std::vec::Vec<usize>,
            ) -> ::fluffr::VerifyResult {
                if depth == 0 {
                    return Err(::fluffr::VerifyError::DepthLimitExceeded);
                }
                ::fluffr::check_bounds(buf, offset, 5, "union inline field")?;
                let jump        = u32::read_at(buf, offset) as usize;
                let tag         = u8::read_at(buf, offset + 4);
                let payload_pos = offset.saturating_add(jump);
                Self::verify_tag(tag, buf, payload_pos, depth, out)
            }
        }

        // ── ReadAt ────────────────────────────────────────────────────────────

        impl<'a, #impl_generics_no_lt> ::fluffr::ReadAt<'a>
            for #enum_name #ty_generics #where_clause
        {
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Union;
            type ReadOutput = #view_enum_name<'a>;

            #[inline(always)]
            fn read_at(_buf: &'a [u8], _offset: usize) -> #view_enum_name<'a> {
                #view_enum_name::default()
            }

            #[inline(always)]
            fn default_output() -> #view_enum_name<'a> { #view_enum_name::default() }

            #[inline(always)]
            fn read_with_tag_at(
                buf:         &'a [u8],
                payload_pos: usize,
                tag:         u8,
            ) -> #view_enum_name<'a> {
                match tag {
                    #(#read_with_tag_arms)*
                    _ => #view_enum_name::default(),
                }
            }
        }

        // ── PartialEq ─────────────────────────────────────────────────────────

        /// Owned ↔ Owned: structural equality, variant + payload.
        impl<#impl_generics_no_lt> PartialEq
            for #enum_name #ty_generics #owned_eq_where
        {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                match (self, other) {
                    #(#owned_self_eq_arms)*
                    _ => false,
                }
            }
        }

        /// Owned ↔ View: structural equality, variant + payload.
        impl<'a> PartialEq<#view_enum_name<'a>> for #enum_name {
            #[inline]
            fn eq(&self, other: &#view_enum_name<'a>) -> bool {
                match (self, other) {
                    #(#view_eq_arms)*
                    _ => false,
                }
            }
        }
        impl<'a> PartialEq<#enum_name> for #view_enum_name<'a> {
            #[inline]
            fn eq(&self, other: &#enum_name) -> bool { other == self }
        }
    })
}


// ── Helpers ───────────────────────────────────────────────────────────────────


fn enum_variants(
    input: &DeriveInput,
) -> &syn::punctuated::Punctuated<syn::Variant, syn::Token![,]> {
    match &input.data {
        Data::Enum(e) => &e.variants,
        _ => unreachable!("already validated"),
    }
}


fn resolve_discriminant(
    enum_name: &syn::Ident,
    vname:     &syn::Ident,
    disc:      &Option<(syn::token::Eq, syn::Expr)>,
    next:      u8,
) -> u8 {
    match disc {
        None => next,
        Some((_, expr)) => {
            if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) = expr {
                i.base10_parse().unwrap_or_else(|_| {
                    panic!("discriminant of `{enum_name}::{vname}` must fit in u8")
                })
            } else {
                panic!("discriminant of `{enum_name}::{vname}` must be a u8 integer literal");
            }
        }
    }
}