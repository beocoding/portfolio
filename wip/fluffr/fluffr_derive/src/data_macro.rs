use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};


pub fn flat(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match &input.data {
        Data::Struct(_) => flat_struct(input),
        Data::Enum(_)   => flat_enum(input),
        Data::Union(_)  => panic!("Flat cannot be derived for unions"),
    }
}


// ── Enum ──────────────────────────────────────────────────────────────────────


fn flat_enum(input: DeriveInput) -> TokenStream {
    let name = &input.ident;

    let repr = get_repr_int(&input.attrs).unwrap_or_else(|| {
        panic!("Flat enums must have a primitive #[repr(...)] attribute (e.g. #[repr(u8)])")
    });

    let expanded = quote! {
        unsafe impl ::bytemuck::Zeroable for #name {}
        unsafe impl ::bytemuck::Pod      for #name {}
        impl ::core::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(stringify!(#name))
                    .finish_non_exhaustive()
            }
        }
        impl ::fluffr::Verify for #name {
            const INLINE_SIZE: usize = ::std::mem::size_of::<#repr>();
            #[inline(always)]
            fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>)
                -> ::fluffr::VerifyResult
            {
                ::fluffr::check_bounds(
                    buf, offset, ::std::mem::size_of::<#repr>(), "flat enum")
            }
        }

        impl #name {
            #[inline(always)]
            pub fn to_le_bytes(self) -> [u8; ::std::mem::size_of::<#repr>()] {
                (self as #repr).to_le_bytes()
            }
            #[inline(always)]
            pub fn from_le_bytes(bytes: &[u8]) -> Self {
                unsafe {
                    ::std::mem::transmute::<#repr, Self>(
                        <#repr>::from_le_bytes(bytes.try_into().unwrap())
                    )
                }
            }

        }

        impl ::core::cmp::PartialEq for #name {
            #[inline(always)]
            fn eq(&self, other: &Self) -> bool {
                (*self as #repr) == (*other as #repr)
            }
        }

        impl ::core::cmp::PartialEq<#repr> for #name {
            #[inline(always)]
            fn eq(&self, other: &#repr) -> bool {
                (*self as #repr) == *other
            }
        }

        impl Serialize for #name {
            const SIZE: usize = size_of::<#repr>();
            const ALIGN: usize = align_of::<#repr>();
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Inline;
            #[inline(always)]
            fn size_hint(&self) -> usize {
                ::std::mem::size_of::<#repr>() + ::std::mem::align_of::<#repr>() - 1
            }
            #[inline(always)]
            fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
                (*self as #repr).write_to(buffer)
            }
            #[inline(always)]
            fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
                (*self as #repr).write_to_unchecked(buffer)
            }
            #[inline(always)]
            fn is_absent(&self) -> bool {
                (*self as #repr) == 0
            }
        }

        impl<'a> ReadAt<'a> for #name {
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Inline;
            type ReadOutput = Self;
            #[inline(always)]
            fn read_at(buf: &[u8], offset: usize) -> Self {
                unsafe {
                    ::std::mem::transmute::<#repr, Self>(
                        <#repr>::read_at(buf, offset)
                    )
                }
            }
            // Inline mode: ListView::get never calls this (the is_offset_flag()
            // guard is false).  Provided defensively so the trait is complete.
            #[inline(always)]
            fn default_output() -> Self {
                unsafe { ::std::mem::transmute::<#repr, Self>(0) }
            }
            #[inline(always)]
            fn payload_block_end(_buf: &'a [u8], pos: usize) -> usize {
                pos + ::std::mem::size_of::<#repr>()
            }
        }
    };

    TokenStream::from(expanded)
}


// ── Struct ────────────────────────────────────────────────────────────────────


fn flat_struct(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(s) => &s.fields,
        _ => unreachable!(),
    };

    let field_idents: Vec<TokenStream2> = match fields {
        Fields::Named(f) => f.named.iter().map(|f| {
            let ident = &f.ident;
            quote! { #ident }
        }).collect(),
        Fields::Unnamed(f) => f.unnamed.iter().enumerate().map(|(i, _)| {
            let idx = syn::Index::from(i);
            quote! { #idx }
        }).collect(),
        Fields::Unit => vec![],
    };

    let field_accessors = field_accessors(fields);

    let expanded = quote! {
        unsafe impl #impl_generics ::bytemuck::Zeroable for #name #ty_generics #where_clause {}
        unsafe impl #impl_generics ::bytemuck::Pod      for #name #ty_generics #where_clause {}

        impl #impl_generics ::core::cmp::PartialEq for #name #ty_generics #where_clause {
            #[inline(always)]
            fn eq(&self, other: &Self) -> bool {
                ::bytemuck::bytes_of(self) == ::bytemuck::bytes_of(other)
            }
        }

        impl #impl_generics ::core::fmt::Debug for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(stringify!(#name))
                    .finish_non_exhaustive()
            }
        }

        impl #impl_generics ::fluffr::Flat for #name #ty_generics #where_clause {
            #[inline(always)]
            fn to_le_bytes(&self) -> ::std::borrow::Cow<'_, [u8]> {
                #[cfg(target_endian = "little")]
                {
                    ::std::borrow::Cow::Borrowed(::bytemuck::bytes_of(self))
                }
                #[cfg(not(target_endian = "little"))]
                {
                    let mut buf = ::std::vec::Vec::<u8>::with_capacity(::std::mem::size_of::<Self>());
                    #(
                        buf.extend_from_slice(&#field_accessors.to_le_bytes());
                    )*
                    ::std::borrow::Cow::Owned(buf)
                }
            }
            #[inline(always)]
            fn from_le_bytes(bytes: &[u8]) -> Self {
                #[cfg(target_endian = "little")]
                {
                    *::bytemuck::from_bytes(bytes)
                }
                #[cfg(not(target_endian = "little"))]
                {
                    let mut buf = <Self as ::bytemuck::Zeroable>::zeroed();
                    let mut offset = 0;
                    #(
                        let size = ::std::mem::size_of_val(&#field_accessors);
                        buf.#field_idents = ::fluffr::Flat::from_le_bytes(&bytes[offset..offset + size]);
                        offset += size;
                    )*
                    buf
                }
            }
        }

        impl #impl_generics ::fluffr::Verify for #name #ty_generics #where_clause {
            const INLINE_SIZE: usize = ::std::mem::size_of::<Self>();
            #[inline(always)]
            fn verify_at(buf: &[u8], offset: usize, _depth: usize, _out: &mut Vec<usize>)
                -> ::fluffr::VerifyResult
            {
                ::fluffr::check_bounds(
                    buf, offset, ::std::mem::size_of::<Self>(), "flat struct")
            }
        }

        impl #impl_generics Serialize for #name #ty_generics #where_clause {
            const SIZE: usize = size_of::<Self>();
            const MODE: ::fluffr::DataType = ::fluffr::DataType::Inline;
            #[inline(always)]
            fn size_hint(&self) -> usize {
                ::std::mem::size_of::<Self>() + ::std::mem::align_of::<Self>() - 1
            }
            #[inline(always)]
            fn write_to<B: Buffer>(&self, buffer: &mut B) -> usize {
                buffer.ensure_capacity(
                    ::std::mem::size_of::<Self>() + ::std::mem::align_of::<Self>() - 1
                );
                self.write_to_unchecked(buffer)
            }
            #[inline(always)]
            fn write_to_unchecked<B: Buffer>(&self, buffer: &mut B) -> usize {
                let size = ::std::mem::size_of::<Self>();
                let mask = ::std::mem::align_of::<Self>() - 1;
                *buffer.head_mut() -= size;
                *buffer.head_mut() &= !mask;
                let head = buffer.head();
                buffer.buffer_mut()[head..head + size]
                    .copy_from_slice(&*self.to_le_bytes());
                buffer.slot()
            }

            #[inline(always)]
            fn is_absent(&self) -> bool {
                *self == <Self as ::bytemuck::Zeroable>::zeroed()
            }
        }

        impl<'a> #impl_generics ReadAt<'a> for #name #ty_generics #where_clause {
            const MODE: DataType = DataType::Inline;
            type ReadOutput = Self;
            #[inline(always)]
            fn read_at(buf: &[u8], offset: usize) -> Self {
                unsafe {
                    let ptr = buf.as_ptr().add(offset) as *const Self;
                    ptr.read_unaligned()
                }
            }
            // Inline mode: ListView::get never calls this (the is_offset_flag()
            // guard is false).  Provided defensively so the trait is complete.
            #[inline(always)]
            fn default_output() -> Self { <Self as ::bytemuck::Zeroable>::zeroed() }
        }

        
    };

    TokenStream::from(expanded)
}


// ── Helpers ───────────────────────────────────────────────────────────────────


fn get_repr_int(attrs: &[syn::Attribute]) -> Option<syn::Ident> {
    const INT_REPRS: &[&str] = &[
        "u8", "u16", "u32", "u64", "u128",
        "i8", "i16", "i32", "i64", "i128",
    ];
    for attr in attrs {
        if !attr.path().is_ident("repr") { continue; }
        let mut found: Option<syn::Ident> = None;
        let _ = attr.parse_nested_meta(|meta| {
            if let Some(ident) = meta.path.get_ident() {
                if INT_REPRS.contains(&ident.to_string().as_str()) {
                    found = Some(ident.clone());
                }
            }
            Ok(())
        });
        if found.is_some() { return found; }
    }
    None
}

fn field_accessors(fields: &Fields) -> Vec<TokenStream2> {
    match fields {
        Fields::Named(f) => f.named.iter().map(|f| {
            let ident = &f.ident;
            quote! { &self.#ident }
        }).collect(),
        Fields::Unnamed(f) => f.unnamed.iter().enumerate().map(|(i, _)| {
            let idx = syn::Index::from(i);
            quote! { &self.#idx }
        }).collect(),
        Fields::Unit => vec![],
    }
}