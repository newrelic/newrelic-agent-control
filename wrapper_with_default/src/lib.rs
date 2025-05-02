use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Path, parse_macro_input, spanned::Spanned};

/// Procedural derive macro to make easier the implementation of wrappers setting a default value.
/// It automatically generates the [Default] implementation with the provided `wrapper_default_value` (which should
/// be a constant pointing to the desired default value) and the [From<T>] implementation to convert from/into
/// the wrapped type.
#[proc_macro_derive(WrapperWithDefault, attributes(wrapper_default_value))]
pub fn wrapper_with_default(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let root_span = input.span(); // Got to improve error handling
    let struct_name = input.ident;

    // Get the type of the wrapped field type
    let Data::Struct(data_struct) = input.data else {
        return syn::Error::new(root_span, "This macro can only be derived for structs")
            .to_compile_error()
            .into();
    };
    let Fields::Unnamed(fields) = data_struct.fields else {
        return syn::Error::new(
            data_struct.struct_token.span(),
            "This macro is only supported for structs with unnamed fields only (tuple-like)",
        )
        .to_compile_error()
        .into();
    };
    if fields.unnamed.len() != 1 {
        return syn::Error::new(
            fields.span(),
            "The struct must have exactly one unnamed field",
        )
        .to_compile_error()
        .into();
    }
    let wrapped_type = &fields.unnamed[0].ty;

    // Get the value of the expected attribute
    let Some(default_value_atrr) = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("wrapper_default_value"))
    else {
        return syn::Error::new(
            root_span,
            "Missing attribute `wrapper_default_value`. Was `#[wrapper_default_value(...)]` set?",
        )
        .to_compile_error()
        .into();
    };

    // Parse the value as a path since it is expected to be a constant
    let Ok(default_value) = default_value_atrr.parse_args::<Path>() else {
        let type_as_string = stringify!(#wrapped_type);
        return syn::Error::new(
            default_value_atrr.span(),
            format!("Expected `wrapper_default_value` to be path literal referencing to a {type_as_string} constant"),
        )
        .to_compile_error()
        .into();
    };

    // Generate the implementation and convert it back to TokenStream
    let expanded = quote! {
        impl From<#wrapped_type> for #struct_name {
            fn from(value: #wrapped_type) -> Self {
                Self(value)
            }
        }

        impl From<#struct_name> for #wrapped_type {
            fn from(value: #struct_name) -> Self {
                value.0
            }
        }

        impl Default for #struct_name {
            fn default() -> Self {
                Self(#default_value)
            }
        }
    };
    expanded.into()
}
