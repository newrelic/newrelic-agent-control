use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Path};

/// Procedural derive macro to make easier the implementation of wrappers setting a default value.
/// It automatically generates the [Default] implementation with the provided `wrapper_default_value` (which should
/// be a constant pointing to the desired default value) and the [From<T>] implementation to convert from/into
/// the wrapped type.
#[proc_macro_derive(WrapperWithDefault, attributes(wrapper_default_value))]
pub fn wrapper_with_default(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = input.ident;

    // Get the value of the 'wrapper_default_value' attribute
    let default_value = input
        .attrs
        .iter()
        .find_map(|attr| {
            if !attr.path().is_ident("wrapper_default_value") {
                return None;
            }
            let value: Path = attr.parse_args().unwrap_or_else(|err| {
                panic!("Expected `wrapper_default_value` to be path literal referencing to a `std::time::Duration` constant: {err}")
            });
            Some(value)
        })
        .expect("Missing attribute `wrapper_default_value`. Was `#[wrapper_default_value(...)]` set?");

    // Get the type of the wrapped field
    let Data::Struct(data_struct) = input.data else {
        panic!("This macro can only be derived for structs");
    };
    let Fields::Unnamed(fields) = data_struct.fields else {
        panic!("The struct must be a tuple struct and have exactly one unnamed field");
    };
    if fields.unnamed.len() != 1 {
        panic!("The struct must have exactly one unnamed field");
    }
    let wrapped_type = &fields.unnamed[0].ty;

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
