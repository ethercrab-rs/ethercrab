use crate::parse_enum::EnumStuff;
use quote::quote;
use std::str::FromStr;
use syn::DeriveInput;

pub fn generate_enum(
    parsed: EnumStuff,
    input: &DeriveInput,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = input.ident.clone();
    let repr_type = parsed.repr_type;

    let primitive_variants = parsed
        .variants
        .clone()
        .into_iter()
        .filter(|variant| !variant.catch_all);

    let (pack, into_primitive_impl) = match &parsed.catch_all {
        Some(_catch_all_variant) => {
            let match_arms = parsed.variants.clone().into_iter().map(|variant| {
                let value =
                    proc_macro2::TokenStream::from_str(&variant.discriminant.to_string()).unwrap();
                let variant_name = variant.name;

                if variant.catch_all {
                    quote! {
                        #name::#variant_name (value) => { *value }
                    }
                } else {
                    quote! {
                        #name::#variant_name => { #value }
                    }
                }
            });

            let match_arms_from = parsed.variants.clone().into_iter().map(|variant| {
                let value =
                    proc_macro2::TokenStream::from_str(&variant.discriminant.to_string()).unwrap();
                let variant_name = variant.name;

                if variant.catch_all {
                    quote! {
                        #name::#variant_name (value) => { value }
                    }
                } else {
                    quote! {
                        #name::#variant_name => { #value }
                    }
                }
            });

            (
                quote! {
                    let value: #repr_type = match self {
                        #(#match_arms),*
                    };

                    buf.copy_from_slice(&value.to_le_bytes());
                },
                quote! {
                    impl From<#name> for #repr_type {
                        fn from(value: #name) -> Self {
                            match value {
                                #(#match_arms_from),*
                            }
                        }
                    }
                },
            )
        }
        None => (
            quote! {
                buf.copy_from_slice(&(*self as #repr_type).to_le_bytes());
            },
            quote! {},
        ),
    };

    let match_arms = primitive_variants.clone().map(|variant| {
        let value = proc_macro2::TokenStream::from_str(&variant.discriminant.to_string()).unwrap();
        let variant_name = variant.name;

        quote! {
            #value => { Ok(Self::#variant_name) }
        }
    });

    let (fallthrough, from_primitive_impl) = match parsed.catch_all.clone() {
        Some(catch_all) => {
            let variant = catch_all.name.clone();
            let catch_all_variant = catch_all.name;

            let fallthrough = quote! {
                other => Ok(Self::#variant(other))
            };

            let from_impl = {
                let match_arms = primitive_variants.clone().map(|variant| {
                    let value =
                        proc_macro2::TokenStream::from_str(&variant.discriminant.to_string())
                            .unwrap();
                    let variant_name = variant.name;

                    quote! {
                        #value => { Self::#variant_name }
                    }
                });

                quote! {
                    impl From<#repr_type> for #name {
                        fn from(value: #repr_type) -> Self {
                            match value {
                                #(#match_arms),*
                                other => Self::#catch_all_variant(other)
                            }
                        }
                    }
                }
            };

            (fallthrough, from_impl)
        }
        None => (
            quote! {
                _other => { Err(::ethercrab_wire::WireError::Todo) }
            },
            // Fallible, so no From<> impl
            quote! {},
        ),
    };

    let out = quote! {
        impl ::ethercrab_wire::EtherCatWire for #name {
            const BYTES: usize = #repr_type::BITS as usize / 8;

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                // TODO: If only one byte, just write it to `buf[0]`
                let mut buf = &mut buf[0..Self::BYTES];

                #pack

                buf
            }

            fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab_wire::WireError> {
                // TODO: If only one byte, just get it from `buf[0]`
                let raw = buf.get(0..Self::BYTES).map(|bytes| {
                    #repr_type::from_le_bytes(bytes.try_into().unwrap())
                }).ok_or(::ethercrab_wire::WireError::Todo)?;

                match raw {
                    #(#match_arms),*
                    #fallthrough,
                }
            }
        }

        #from_primitive_impl
        #into_primitive_impl
    };

    Ok(out)
}
