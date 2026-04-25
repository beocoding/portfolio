use std::cmp::Reverse;
use proc_macro2::{TokenStream as TokenStream2, TokenTree};
use syn::{Data, DeriveInput, Ident};
pub(crate) use quote::{format_ident, quote};

// ── Field classification ──────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum FieldCategory {
    List(Box<FieldCategory>),
    Union,
    Table,
    FileBlob,
    String,
    Inline,
}

impl FieldCategory {
    pub fn complexity(&self) -> u32 {
        match self {
            FieldCategory::Inline      => 0,
            FieldCategory::Table       => 1,
            FieldCategory::Union       => 2,
            FieldCategory::String      => 3,
            FieldCategory::FileBlob => 3,
            FieldCategory::List(inner) => 4 + inner.complexity(),
        }
    }
    pub fn is_indirect(&self) -> bool { !matches!(self, FieldCategory::Inline) }

    pub fn can_key(&self) -> bool {matches!(self, FieldCategory::Inline | FieldCategory::String)}
}

// ── Per-field metadata ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct FieldMeta {
    pub accessor:    TokenStream2,
    pub offset:      usize,
    pub label:       Ident,
    pub category:    FieldCategory,
    pub default_val: Option<TokenStream2>,
    pub ty:          syn::Type,
    pub key:         bool,
}

impl FieldMeta {
    pub fn slot_var(&self) -> Ident { format_ident!("slot_{}", self.label) }

    pub fn is_absent_check(&self) -> TokenStream2 {
        let a = &self.accessor;
        if let Some(default) = &self.default_val {
            return quote! { (#a == #default) };
        }
        match &self.category {
            FieldCategory::FileBlob | FieldCategory::String | FieldCategory::List(_) => quote! { #a.is_empty() },
            FieldCategory::Union  => quote! { #a.is_absent() },
            FieldCategory::Table  => quote! { #a.is_absent() },
            FieldCategory::Inline => quote! { #a.to_le_bytes().as_ref().iter().all(|&b| b == 0) },
        }
    }
}

// ── Table metadata ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TableMeta {
    pub label:            Ident,
    pub field_metas:      Vec<FieldMeta>,
    pub complexity_index: Vec<usize>,
    pub vtable_size:      usize,
    pub merge:            bool,
}

impl TableMeta {
    pub fn new(label: Ident, field_metas: Vec<FieldMeta>, vtable_size: usize) -> Self {
        let mut complexity_index: Vec<usize> = (0..field_metas.len()).collect();
        complexity_index.sort_by_key(|&i| Reverse(field_metas[i].category.complexity()));
        let merge = field_metas.iter().all(|f| matches!(f.category, FieldCategory::List(_)));
        Self { label, field_metas, complexity_index, vtable_size, merge }
    }
    pub fn iter_fields(&self)        -> impl Iterator<Item=&FieldMeta> { self.field_metas.iter() }
    pub fn iter_fields_rev(&self)    -> impl Iterator<Item=&FieldMeta> { self.field_metas.iter().rev() }
    pub fn iter_by_complexity(&self) -> impl Iterator<Item=&FieldMeta> {
        self.complexity_index.iter().map(|&i| &self.field_metas[i])
    }
    pub fn iter_by_simplicity(&self) -> impl Iterator<Item=&FieldMeta> {
        self.complexity_index.iter().rev().map(|&i| &self.field_metas[i])
    }
}

// ── Parsing ───────────────────────────────────────────────────────────────────
pub fn analyze_meta(input: DeriveInput) -> TableMeta {
    let label = input.ident.clone();
    let fields = match &input.data {
        Data::Struct(s) => &s.fields,
        _ => panic!("FlatTable derive only supports structs"),
    };

    let mut next_slot = 0usize;
    let field_metas: Vec<FieldMeta> = fields.iter().enumerate().map(|(i, field)| {
        let accessor = match &field.ident {
            Some(ident) => quote! { self.#ident },
            None        => { let idx = syn::Index::from(i); quote! { self.#idx } }
        };

        let key = field.attrs.iter().any(|attr| attr.path().is_ident("key"));

        let explicit = field.attrs.iter().find_map(|attr| {
            let name = attr.path().get_ident()?.to_string();
            if matches!(name.as_str(), "key" | "default") { return None; }
            let args = attr.parse_args::<TokenStream2>().ok();
            Some(category_from(&name, args))
        });

        let category = resolve_category(explicit, &field.ty);
        let slot = next_slot;
        next_slot += 1;

        FieldMeta {
            accessor,
            offset: slot,
            label: field.ident.clone().unwrap_or_else(|| format_ident!("field_{}", i)),
            default_val: field.attrs.iter().find_map(|attr| {
                if !attr.path().is_ident("default") { return None; }
                let expr = attr.parse_args_with(|input: syn::parse::ParseStream| {
                    let _ = input.parse::<syn::Token![=]>();
                    input.parse::<syn::Expr>()
                }).expect("#[default] must be in the form #[default = <expr>]");
                Some(quote! { #expr })
            }),
            category,
            ty: field.ty.clone(),
            key,
        }
    }).collect();

    let vtable_size = next_slot * 2 + 4;
    TableMeta::new(label, field_metas, vtable_size)
}
pub fn category_from(name: &str, args: Option<TokenStream2>) -> FieldCategory {
    match name {
        "table"             => FieldCategory::Table,
        "file"              => FieldCategory::FileBlob,
        "string"            => FieldCategory::String,
        "inline" | "scalar" => FieldCategory::Inline,
        "union"             => FieldCategory::Union,
        "array" => {
            let inner = args.and_then(|ts| {
                let mut iter = ts.into_iter();
                let ident = match iter.next() {
                    Some(TokenTree::Ident(i)) => i,
                    _ => return None,
                };
                let inner_args = match iter.next() {
                    Some(TokenTree::Group(g)) => Some(g.stream()),
                    _ => None,
                };
                Some(category_from(&ident.to_string(), inner_args))
            }).unwrap_or(FieldCategory::Inline);
            FieldCategory::List(Box::new(inner))
        }
        _ => FieldCategory::Inline,
    }
}

/// Convert a Row field's category into the `#[array(<kind>)]` attribute that the
/// generated Registry struct needs on the corresponding `Vec<T>` field so that
/// `#[derive(Table)]` on the Registry struct produces the correct layout.
pub fn category_to_array_attr(cat: &FieldCategory) -> TokenStream2 {
    match cat {
        FieldCategory::Inline   => quote! { #[array(scalar)] },
        FieldCategory::String   => quote! { #[array(string)] },
        FieldCategory::FileBlob => quote! { #[array(file)]   },
        FieldCategory::Table    => quote! { #[array(table)]  },
        FieldCategory::Union    => quote! { #[array(union)]  },
        FieldCategory::List(_)  => unreachable!("as_row already rejects List fields"),
    }
}

// ── Type inference ────────────────────────────────────────────────────────────

pub fn type_is_string(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last()
            .map(|s| s.ident == "String").unwrap_or(false),
        syn::Type::Reference(r) => {
            if let syn::Type::Path(tp) = r.elem.as_ref() {
                tp.path.segments.last().map(|s| s.ident == "str").unwrap_or(false)
            } else { false }
        }
        _ => false,
    }
}

pub fn type_is_fileblob(ty: &syn::Type) -> bool {
    if let syn::Type::Path(tp) = ty {
        tp.path.segments.last().map(|seg| seg.ident == "FileBlob") == Some(true)
    } else { false }
}

pub fn vec_inner(ty: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            if seg.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = ab.args.first() {
                        return Some(inner);
                    }
                }
            }
        }
    }
    None
}

pub fn infer_category(ty: &syn::Type) -> FieldCategory {
    if type_is_string(ty) { return FieldCategory::String; }
    if type_is_fileblob(ty) { return FieldCategory::FileBlob; }
    if let Some(inner) = vec_inner(ty) {
        let inner_cat = if type_is_string(inner) { FieldCategory::String } else if type_is_fileblob(inner) {FieldCategory::FileBlob} else { FieldCategory::Inline };
        return FieldCategory::List(Box::new(inner_cat));
    }
    FieldCategory::Inline
}

pub fn resolve_category(explicit: Option<FieldCategory>, ty: &syn::Type) -> FieldCategory {
    match explicit {
        None => infer_category(ty),
        Some(FieldCategory::List(inner)) => FieldCategory::List(inner),
        Some(other) => {
            if vec_inner(ty).is_some() { FieldCategory::List(Box::new(other)) } else { other }
        }
    }
}
