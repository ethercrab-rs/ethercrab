use crate::parse_struct::StructStuff;
use quote::quote;
use std::str::FromStr;
use syn::DeriveInput;

pub fn generate_struct(
    parsed: StructStuff,
    input: &DeriveInput,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = input.ident.clone();

    let width_bytes = parsed.width.div_ceil(8);

    let fields_pack = parsed.fields.clone().into_iter().map(|field| {
        let ty = field.ty;
        let name = field.name;

        let byte_start = field.bytes.start;

        let bit_start = field.bit_offset;

        if field.bits.len() <= 8 {
            let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
            let mask = proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

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

    let fields_unpack = parsed.fields.clone().into_iter().map(|field| {
                let ty = field.ty;
                let name = field.name;

                let byte_start = field.bytes.start;

                let bit_start = field.bit_offset;

                if field.bits.len() <= 8 {
                    let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
                    let mask =
                        proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

                    if field.is_enum {
                        quote! {
                            #name: {
                                let masked = (buf[#byte_start] & #mask) >> #bit_start;

                                <#ty as num_enum::TryFromPrimitive>::try_from_primitive(masked)
                                    .map_err(|_| ::ethercrab::error::Error::Internal)?
                            },
                        }
                    } else if field.ty_name == "bool" {
                        quote! {
                            #name: ((buf[#byte_start] & #mask) >> #bit_start) > 0,
                        }
                    } else {
                        quote! {
                            #name: (buf[#byte_start] & #mask) >> #bit_start,
                        }
                    }
                }
                // Assumption: multi-byte fields are byte-aligned. This should be validated during
                // parse.
                else {
                    let start_byte = field.bytes.start;
                    let end_byte = field.bytes.end;

                    if field.is_enum {
                        quote! {
                            #name: <#ty as num_enum::TryFromPrimitive>::try_from_primitive(<#ty as ::ethercrab::derive::WireFieldEnum>::unpack_to_repr(&buf))
                                .map_err(|_| ::ethercrab::error::Error::Internal)?,
                        }
                    } else {
                        quote! {
                            #name: <#ty as ::ethercrab::derive::WireField>::unpack_from_slice(&buf[#start_byte..#end_byte])?,
                        }
                    }
                }
            });

    let out = quote! {
        impl ::ethercrab::derive::WireField for #name {
            // const BITS: usize = #width_bits;
            const BYTES: usize = #width_bytes;

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                #(#fields_pack)*

                &buf[0..#width_bytes]
            }

            fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab::error::Error> {
                if buf.len() < Self::BYTES {
                    return Err(::ethercrab::error::Error::Internal)
                }

                Ok(Self {
                    #(#fields_unpack)*
                })
            }
        }
    };

    Ok(out)
}
