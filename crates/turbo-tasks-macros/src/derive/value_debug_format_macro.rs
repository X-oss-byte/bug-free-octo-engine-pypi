use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, DeriveInput, Field, FieldsNamed, FieldsUnnamed};
use turbo_tasks_macros_shared::{generate_destructuring, match_expansion};

use super::FieldAttributes;

fn ignore_field(field: &Field) -> bool {
    FieldAttributes::from(field.attrs.as_slice()).debug_ignore
}

/// This macro generates the implementation of the `ValueDebugFormat` trait for
/// a given type.
///
/// Fields annotated with `#[debug_ignore]` will not appear in the
/// `ValueDebugFormat` representation of the type.
pub fn derive_value_debug_format(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);

    let ident = &derive_input.ident;
    let formatting_logic =
        match_expansion(&derive_input, &format_named, &format_unnamed, &format_unit);

    let value_debug_format_ident = get_value_debug_format_ident(ident);

    quote! {
        impl #ident {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            async fn #value_debug_format_ident(&self, depth: usize) -> anyhow::Result<turbo_tasks::debug::ValueDebugStringVc> {
                if depth == 0 {
                    return Ok(turbo_tasks::debug::ValueDebugStringVc::new(stringify!(#ident).to_string()));
                }

                use turbo_tasks::debug::internal::*;
                use turbo_tasks::debug::ValueDebugFormat;
                Ok(turbo_tasks::debug::ValueDebugStringVc::new(format!("{:#?}", #formatting_logic)))
            }
        }

        impl turbo_tasks::debug::ValueDebugFormat for #ident {
            fn value_debug_format<'a>(&'a self, depth: usize) -> turbo_tasks::debug::ValueDebugFormatString<'a> {
                turbo_tasks::debug::ValueDebugFormatString::Async(
                    Box::pin(async move {
                        Ok(self.#value_debug_format_ident(depth).await?.await?.to_string())
                    })
                )
            }
        }
    }
    .into()
}

/// Formats a single field nested inside named or unnamed fields.
fn format_field(value: TokenStream2) -> TokenStream2 {
    quote! {
        match #value.value_debug_format(depth.saturating_sub(1)).try_to_value_debug_string().await {
            Ok(result) => match result.await {
                Ok(result) => result.to_string(),
                Err(err) => format!("{:?}", err),
            },
            Err(err) => format!("{:?}", err),
        }
    }
}

/// Formats a struct or enum variant with named fields (e.g. `struct Foo {
/// bar: u32 }`, `Foo::Bar { baz: u32 }`).
fn format_named(ident: &Ident, fields: &FieldsNamed) -> (TokenStream2, TokenStream2) {
    let (captures, fields_idents) = generate_destructuring(fields.named.iter(), &ignore_field);
    let fields_values = fields_idents.iter().cloned().map(format_field);
    (
        captures,
        quote! {
            FormattingStruct::new_named(
                stringify!(#ident),
                vec![#(
                    FormattingField::new(
                        stringify!(#fields_idents),
                        #fields_values,
                    ),
                )*],
            )
        },
    )
}

/// Formats a struct or enum variant with unnamed fields (e.g. `struct
/// Foo(u32)`, `Foo::Bar(u32)`).
fn format_unnamed(ident: &Ident, fields: &FieldsUnnamed) -> (TokenStream2, TokenStream2) {
    let (captures, fields_idents) = generate_destructuring(fields.unnamed.iter(), &ignore_field);
    let fields_values = fields_idents.into_iter().map(format_field);
    (
        captures,
        quote! {
            FormattingStruct::new_unnamed(
                stringify!(#ident),
                vec![#(
                    #fields_values,
                )*],
            )
        },
    )
}

/// Formats a unit struct or enum variant (e.g. `struct Foo;`, `Foo::Bar`).
fn format_unit(ident: &Ident) -> (TokenStream2, TokenStream2) {
    (
        quote! {},
        quote! {
            FormattingStruct::new_unnamed(
                stringify!(#ident),
                vec![],
            )
        },
    )
}

pub(crate) fn get_value_debug_format_ident(ident: &Ident) -> Ident {
    Ident::new(&format!("__value_debug_format_{}", ident), ident.span())
}
