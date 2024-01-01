mod derive_enum;
mod derive_struct;
mod help;

use derive_enum::parse_enum;
use derive_struct::parse_struct;
use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse::ParseStream, parse_macro_input, punctuated::Punctuated, Attribute, Data, DataEnum,
    DataStruct, DeriveInput, Expr, ExprLit, Lit, Meta, MetaNameValue, Token,
};

#[proc_macro_derive(EtherCatWire, attributes(wire))]
pub fn ethercat_wire(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    let res = match input.clone().data {
        Data::Enum(e) => parse_enum(e, input.clone()).map(|parsed| {
            let vis = input.vis;
            let name = input.ident;

            let width_bits = parsed.width;
            let width_bytes = parsed.width.div_ceil(8);

            quote! {
                impl ::ethercrab::derive::WireEnum for #name {
                    const BITS: usize = #width_bits;
                    const BYTES: usize = #width_bytes;
                }
            }
        }),
        Data::Struct(s) => parse_struct(s, input.clone()).map(|parsed| {
            let vis = input.vis;
            let name = input.ident;

            let width_bits = parsed.width;
            let width_bytes = parsed.width.div_ceil(8);

            quote! {
                impl ::ethercrab::derive::WireStruct for #name {
                    const BITS: usize = #width_bits;
                    const BYTES: usize = #width_bytes;
                }
            }
        }),
        Data::Union(_) => Err(syn::Error::new(
            input.ident.span(),
            "Unions are not supported",
        )),
    };

    let res = match res {
        Ok(res) => res,
        Err(e) => return e.to_compile_error().into(),
    };

    TokenStream::from(res)
}

#[cfg(test)]
mod tests {
    #[test]
    fn trybuild_cases() {
        let t = trybuild::TestCases::new();

        t.compile_fail("ui/*.rs");
    }
}
