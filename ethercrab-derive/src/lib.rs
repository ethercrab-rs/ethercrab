mod derive_enum;
mod derive_struct;
mod help;

use derive_enum::parse_enum;
use derive_struct::parse_struct;
use proc_macro::TokenStream;
use quote::quote;
use std::str::FromStr;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(EtherCatWire, attributes(wire))]
pub fn ethercat_wire(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    let res = match input.clone().data {
        Data::Enum(e) => parse_enum(e, input.clone()).map(|_parsed| {
            quote! {}
        }),
        Data::Struct(s) => parse_struct(s, input.clone()).map(|parsed| {
            let name = input.ident;

            let width_bytes = parsed.width.div_ceil(8);

            let fields = parsed.fields.into_iter().map(|field| {
                let ty = field.ty;
                let name = field.name;

                let byte_start = field.bytes.start;

                let bit_start = field.bit_offset;

                if field.bits.len() < 8 {
                    let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
                    let mask =
                        proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

                    quote! {
                        buf[#byte_start] |= ((self.#name as u8) << #bit_start) & #mask;
                    }
                }
                // Assumption: multi-byte fields are byte-aligned. This should be validated during
                // parse.
                else {
                    let start_byte = field.bytes.start;
                    let end_byte = field.bytes.end;

                    if field.is_enum {
                        quote! {
                            buf[#start_byte..#end_byte].copy_from_slice(&(self.#name as #ty).to_le_bytes());
                        }
                    } else {
                        quote! {
                            self.#name.pack_to_slice_unchecked(&mut buf[#start_byte..#end_byte]);
                        }
                    }
                }
            });

            quote! {
                impl ::ethercrab::derive::WireField for #name {
                    // const BITS: usize = #width_bits;
                    const BYTES: usize = #width_bytes;

                    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                        #(#fields)*

                        &buf[0..#width_bytes]
                    }
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
