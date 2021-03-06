/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
/*!
    This crate contains the internal procedural macros
    used by the sixtyfps corelib crate
*/

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_derive(BuiltinItem, attributes(rtti_field))]
pub fn builtin_item(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    let fields = match &input.data {
        syn::Data::Struct(syn::DataStruct { fields: f @ syn::Fields::Named(..), .. }) => f,
        _ => {
            return syn::Error::new(
                input.ident.span(),
                "Only `struct` with named field are supported",
            )
            .to_compile_error()
            .into()
        }
    };

    let (prop_field_names, prop_field_types): (Vec<_>, Vec<_>) = fields
        .iter()
        .filter(|f| is_property(&f.ty) && matches!(f.vis, syn::Visibility::Public(_)))
        .map(|f| (f.ident.as_ref().unwrap(), &f.ty))
        .unzip();

    let (plain_field_names, plain_field_types): (Vec<_>, Vec<_>) = fields
        .iter()
        .filter(|f| {
            f.attrs
                .iter()
                .find(|attr| {
                    attr.parse_meta()
                        .ok()
                        .map(|meta| match meta {
                            syn::Meta::Path(path) => path
                                .get_ident()
                                .map(|ident| ident.to_string() == "rtti_field")
                                .unwrap_or(false),
                            _ => false,
                        })
                        .unwrap_or(false)
                })
                .is_some()
        })
        .map(|f| (f.ident.as_ref().unwrap(), &f.ty))
        .unzip();

    let callback_field_names =
        fields.iter().filter(|f| is_callback(&f.ty)).map(|f| f.ident.as_ref().unwrap());

    let item_name = &input.ident;

    quote!(
        #[cfg(feature = "rtti")]
        impl BuiltinItem for #item_name {
            fn name() -> &'static str {
                stringify!(#item_name)
            }
            fn properties<Value: ValueType>() -> Vec<(&'static str, &'static dyn PropertyInfo<Self, Value>)> {
                vec![#( {
                    const O : MaybeAnimatedPropertyInfoWrapper<#item_name, #prop_field_types> =
                        MaybeAnimatedPropertyInfoWrapper(#item_name::FIELD_OFFSETS.#prop_field_names);
                    (stringify!(#prop_field_names), (&O).as_property_info())
                } ),*]
            }
            fn fields<Value: ValueType>() -> Vec<(&'static str, &'static dyn FieldInfo<Self, Value>)> {
                vec![#( {
                    const O : const_field_offset::FieldOffset<#item_name, #plain_field_types, const_field_offset::AllowPin> =
                        #item_name::FIELD_OFFSETS.#plain_field_names;
                    (stringify!(#plain_field_names), &O as &'static dyn FieldInfo<Self, Value> )
                } ),*]
            }
            fn callbacks() -> Vec<(&'static str, const_field_offset::FieldOffset<Self, Callback<()>, const_field_offset::AllowPin>)> {
                vec![#(
                    (stringify!(#callback_field_names),#item_name::FIELD_OFFSETS.#callback_field_names)
                ),*]
            }
        }
    )
    .into()
}

fn type_name(ty: &syn::Type) -> String {
    quote!(#ty).to_string()
}

fn is_property(ty: &syn::Type) -> bool {
    type_name(ty).starts_with("Property <")
}
fn is_callback(ty: &syn::Type) -> bool {
    type_name(ty).to_string().starts_with("Callback <")
}

#[proc_macro_derive(MappedKeyCode)]
pub fn keycode_mapping(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    let variants = match &input.data {
        syn::Data::Enum(syn::DataEnum { variants, .. }) => variants,
        _ => {
            return syn::Error::new(input.ident.span(), "Only `enum` types are supported")
                .to_compile_error()
                .into()
        }
    }
    .iter()
    .collect::<Vec<_>>();

    quote!(
        impl From<winit::event::VirtualKeyCode> for KeyCode {
            fn from(code: winit::event::VirtualKeyCode) -> Self {
                match code {
                    #(winit::event::VirtualKeyCode::#variants => Self::#variants),*
                }
            }
        }
    )
    .into()
}
