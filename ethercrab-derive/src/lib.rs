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
        Data::Struct(s) => todo!(),
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

struct EnumStuff {
    /// Width in bits on the wire.
    width: usize,
}

fn parse_enum(
    e: DataEnum,
    DeriveInput { attrs, ident, .. }: DeriveInput,
) -> syn::Result<EnumStuff> {
    // dbg!(&attrs);
    // dbg!(e);

    let mut width = None;

    for attr in attrs {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                syn::Meta::Path(_) => (),
                syn::Meta::List(_) => (),
                syn::Meta::NameValue(nv) if nv.path.is_ident("bits") => {
                    if let syn::Expr::Lit(ExprLit {
                        lit: Lit::Int(lit), ..
                    }) = &nv.value
                    {
                        width = Some(lit.base10_parse::<usize>()?);
                    }
                }
                syn::Meta::NameValue(nv) => {
                    dbg!("Ignore attribute {:?}", nv.path.get_ident());
                }
            }
        }
    }

    let Some(width) = width else {
        return Err(syn::Error::new(
            ident.span(),
            "Enum bit width is required, e.g. #[wire(bits = 8)]",
        ));
    };

    dbg!(width);

    Ok(EnumStuff { width })
}

#[cfg(test)]
mod tests {
    #[test]
    fn trybuild_cases() {
        let t = trybuild::TestCases::new();

        t.compile_fail("ui/*.rs");
    }
}
