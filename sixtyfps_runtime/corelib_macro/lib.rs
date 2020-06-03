/*!
    This crate contains the internal procedural macros
    used by the sixtyfps corelib crate
*/

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_derive(BuiltinItem)]
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
        .filter(|f| is_property(&f.ty))
        .map(|f| (f.ident.as_ref().unwrap(), &f.ty))
        .unzip();

    let signal_field_names =
        fields.iter().filter(|f| is_signal(&f.ty)).map(|f| f.ident.as_ref().unwrap());

    let item_name = &input.ident;

    quote!(
        //#[cfg(feature = "rtti")]
        impl BuiltinItem for #item_name {
            fn name() -> &'static str {
                stringify!(#item_name)
            }
            fn properties<Value: ValueType>() -> Vec<(&'static str, &'static dyn PropertyInfo<Self, Value>)> {
                vec![#( {
                    const O : FieldOffset<#item_name, #prop_field_types> =
                        #item_name::field_offsets().#prop_field_names;
                    (stringify!(#prop_field_names), &O as &'static dyn PropertyInfo<Self, Value> )
                } ),*]
            }
            fn signals() -> Vec<(&'static str, FieldOffset<Self, crate::Signal<()>>)> {
                vec![#(
                    (stringify!(#signal_field_names),#item_name::field_offsets().#signal_field_names)
                ),*]
            }
        }
    )
    .into()
}

fn is_property(ty: &syn::Type) -> bool {
    quote!(#ty).to_string().starts_with("Property <")
}
fn is_signal(ty: &syn::Type) -> bool {
    quote!(#ty).to_string().starts_with("Signal <")
}