//! Derive macros for `autobot-kernel`.
//!
//! `#[derive(FieldClasses)]` implements `autobot_kernel::fields::FieldClasses` for a struct
//! with named fields. Every field carries exactly one `#[field(...)]` attribute:
//!
//! - `#[field(domain)]`, `#[field(control)]`, `#[field(reconciliation)]` or
//!   `#[field(structural)]`: the field is a leaf of that class, compared with `PartialEq`.
//! - `#[field(nested)]`: the field's type implements `FieldClasses` and classifies its own
//!   parts; a change to the value's shape (an `Option` becoming present, an entry added to or
//!   removed from a `Vec` or `BTreeMap`) counts as domain.
//! - `#[field(<class>, nested)]`: as `nested`, with shape changes counting as `<class>`.
//!
//! A field without the attribute, an unknown word, two classes or a repeated attribute is a
//! compile error. A field marked `#[serde(flatten)]` adds no segment to the partition paths
//! of its parts. The generated code names the kernel as `autobot_kernel`, so inside the kernel
//! itself `use crate as autobot_kernel;` must be in scope.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::quote;
use syn::{Data, DeriveInput, Error, Field, Fields, Ident, parse_macro_input};

/// Derives `autobot_kernel::fields::FieldClasses`; see the crate documentation.
#[proc_macro_derive(FieldClasses, attributes(field))]
pub fn derive_field_classes(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// A field's four classes, as the attribute spells them.
const CLASSES: [&str; 4] = ["domain", "control", "reconciliation", "structural"];

/// What the `#[field(...)]` attribute says about one field.
struct FieldSpec {
    /// The class of the field, or of its shape when `nested`.
    class: Ident,
    /// Whether the field's type classifies its own parts.
    nested: bool,
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "FieldClasses supports structs with named fields only",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                "FieldClasses supports structs with named fields only",
            ));
        }
    };

    let mut diff = Vec::new();
    let mut classes = Vec::new();
    let mut partition = Vec::new();
    let mut errors: Option<Error> = None;
    for field in fields {
        let spec = match field_spec(field) {
            Ok(spec) => spec,
            Err(e) => {
                match &mut errors {
                    Some(all) => all.combine(e),
                    None => errors = Some(e),
                }
                continue;
            }
        };
        let Some(ident) = &field.ident else {
            continue;
        };
        let ty = &field.ty;
        let class = &spec.class;
        let class = quote!(autobot_kernel::fields::FieldClass::#class);
        let name = ident.to_string();
        let path = if is_flattened(field) {
            quote!(prefix.clone())
        } else {
            quote!(prefix.field(#name))
        };
        if spec.nested {
            diff.push(quote! {
                autobot_kernel::fields::FieldClasses::diff(
                    &self.#ident, &other.#ident, #class, touched,
                );
            });
            classes.push(quote! {
                autobot_kernel::fields::FieldClasses::classes(&self.#ident, #class, touched);
            });
            partition.push(quote! {
                <#ty as autobot_kernel::fields::FieldClasses>::partition(&#path, #class, out);
            });
        } else {
            diff.push(quote! {
                if self.#ident != other.#ident {
                    touched.insert(#class);
                }
            });
            classes.push(quote!(touched.insert(#class);));
            partition.push(quote!(out.push((#path, #class));));
        }
    }
    if let Some(e) = errors {
        return Err(e);
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics autobot_kernel::fields::FieldClasses for #name #ty_generics
        #where_clause
        {
            #[allow(unused_variables)]
            fn diff(
                &self,
                other: &Self,
                shape: autobot_kernel::fields::FieldClass,
                touched: &mut autobot_kernel::fields::ClassSet,
            ) {
                #(#diff)*
            }

            #[allow(unused_variables)]
            fn classes(
                &self,
                shape: autobot_kernel::fields::FieldClass,
                touched: &mut autobot_kernel::fields::ClassSet,
            ) {
                #(#classes)*
            }

            #[allow(unused_variables)]
            fn partition(
                prefix: &autobot_kernel::fields::FieldPath,
                shape: autobot_kernel::fields::FieldClass,
                out: &mut ::std::vec::Vec<(
                    autobot_kernel::fields::FieldPath,
                    autobot_kernel::fields::FieldClass,
                )>,
            ) {
                #(#partition)*
            }
        }
    })
}

/// Reads the one `#[field(...)]` attribute of `field`.
fn field_spec(field: &Field) -> syn::Result<FieldSpec> {
    let mut found: Option<FieldSpec> = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("field")) {
        if found.is_some() {
            return Err(Error::new_spanned(
                attr,
                "a field carries one #[field(...)] attribute",
            ));
        }
        let mut class: Option<Ident> = None;
        let mut nested = false;
        attr.parse_nested_meta(|meta| {
            let Some(word) = meta.path.get_ident() else {
                return Err(meta.error(expected()));
            };
            if word == "nested" {
                if nested {
                    return Err(meta.error("`nested` is given twice"));
                }
                nested = true;
                return Ok(());
            }
            let Some(variant) = class_variant(word) else {
                return Err(meta.error(expected()));
            };
            if class.is_some() {
                return Err(meta.error("a field has one class"));
            }
            class = Some(Ident::new(variant, word.span()));
            Ok(())
        })?;
        let class = match (class, nested) {
            (Some(class), _) => class,
            (None, true) => Ident::new("Domain", Span::call_site()),
            (None, false) => return Err(Error::new_spanned(attr, expected())),
        };
        found = Some(FieldSpec { class, nested });
    }
    found.ok_or_else(|| {
        let what = field
            .ident
            .as_ref()
            .map_or_else(|| "this field".to_owned(), |i| format!("field `{i}`"));
        Error::new_spanned(
            field
                .ident
                .as_ref()
                .map_or_else(TokenStream2::new, |i| quote!(#i)),
            format!(
                "{what} has no #[field(...)] class; every status field is one of {}, \
                 or `nested` (KERNEL §1)",
                CLASSES.join(", ")
            ),
        )
    })
}

/// The `FieldClass` variant for an attribute word.
fn class_variant(word: &Ident) -> Option<&'static str> {
    match word.to_string().as_str() {
        "domain" => Some("Domain"),
        "control" => Some("Control"),
        "reconciliation" => Some("Reconciliation"),
        "structural" => Some("Structural"),
        _ => None,
    }
}

/// The message for an attribute that names no known class.
fn expected() -> String {
    format!(
        "expected one of {}, optionally with `nested`, or `nested` alone",
        CLASSES.join(", ")
    )
}

/// Whether `field` carries `#[serde(flatten)]`.
fn is_flattened(field: &Field) -> bool {
    field
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("serde"))
        .filter_map(|a| a.meta.require_list().ok())
        .any(|list| {
            list.tokens
                .clone()
                .into_iter()
                .any(|t| matches!(t, TokenTree::Ident(i) if i == "flatten"))
        })
}
