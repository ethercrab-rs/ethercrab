use crate::parse_enum::EnumStuff;
use quote::quote;
use std::{f32::consts::E, str::FromStr};
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

    let pack = if parsed.catch_all.is_some() {
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

        quote! {
            let value: #repr_type = match self {
                #(#match_arms),*
            };

            buf.copy_from_slice(&value.to_le_bytes());
        }
    } else {
        quote! {
            buf.copy_from_slice(&(*self as #repr_type).to_le_bytes());
        }
    };

    let into_primitive_impl = if parsed.catch_all.is_some() {
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

        quote! {
            impl From<#name> for #repr_type {
                fn from(value: #name) -> Self {
                    match value {
                        #(#match_arms_from),*
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    let match_arms = primitive_variants.clone().map(|variant| {
        let value = proc_macro2::TokenStream::from_str(&variant.discriminant.to_string()).unwrap();
        let variant_name = variant.name;

        quote! {
            #value => { Self::#variant_name }
        }
    });

    let result_match_arms = primitive_variants.clone().map(|variant| {
        let value = proc_macro2::TokenStream::from_str(&variant.discriminant.to_string()).unwrap();
        let variant_name = variant.name;

        quote! {
            #value => { Ok(Self::#variant_name) }
        }
    });

    let fallthrough = if let Some(ref catch_all_variant) = parsed.catch_all {
        let catch_all = catch_all_variant.name.clone();

        quote! {
            other => Ok(Self::#catch_all(other))
        }
    } else if let Some(ref default_variant) = parsed.default_variant {
        let default = default_variant.name.clone();

        quote! {
            _other => Ok(Self::#default)
        }
    } else {
        quote! {
            _other => { Err(::ethercrab_wire::WireError::Todo) }
        }
    };

    let from_primitive_impl = if let Some(catch_all_variant) = parsed.catch_all {
        let catch_all = catch_all_variant.name.clone();
        let match_arms = match_arms.clone();

        quote! {
            impl From<#repr_type> for #name {
                fn from(value: #repr_type) -> Self {
                    match value {
                        #(#match_arms),*
                        other => Self::#catch_all(other)
                    }
                }
            }
        }
    } else if let Some(default_variant) = parsed.default_variant {
        let default = default_variant.name;
        let match_arms = match_arms.clone();

        quote! {
            impl From<#repr_type> for #name {
                fn from(value: #repr_type) -> Self {
                    match value {
                        #(#match_arms),*
                        other => Self::#default
                    }
                }
            }
        }
    } else {
        let match_arms = result_match_arms.clone();

        quote! {
            impl TryFrom<#repr_type> for #name {
                type Error = ::ethercrab_wire::WireError;

                fn try_from(value: #repr_type) -> Result<Self, Self::Error> {
                    match value {
                        #(#match_arms),*
                        _other => Err(::ethercrab_wire::WireError::Todo)
                    }
                }
            }
        }
    };

    let size_bytes = match repr_type.to_string().as_str() {
        "u8" => 1usize,
        "u16" => 2,
        "u32" => 4,
        invalid => unreachable!("Invalid repr {}", invalid),
    };

    let out = quote! {
        impl ::ethercrab_wire::EtherCrabWireWrite for #name {
            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let mut buf = &mut buf[0..#size_bytes];

                #pack

                buf
            }

            fn packed_len(&self) -> usize {
                #size_bytes
            }
        }

        impl ::ethercrab_wire::EtherCrabWireRead for #name {
            // fn unpack_from_slice_rest<'buf>(buf: &'buf [u8]) -> Result<(Self, &'buf [u8]), ::ethercrab_wire::WireError> {
            //     if buf.len() < #size_bytes {
            //         return Err(::ethercrab_wire::WireError::Todo)
            //     }

            //     let (buf, rest) = buf.split_at(#size_bytes);

            //     match #repr_type::from_le_bytes(buf.try_into().unwrap()) {
            //         #(#result_match_arms),*
            //         #fallthrough,
            //     }.map(|out| (out, rest))
            // }
            fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab_wire::WireError> {
                let raw = buf.get(0..#size_bytes).map(|bytes| {
                    #repr_type::from_le_bytes(bytes.try_into().unwrap())
                }).ok_or(::ethercrab_wire::WireError::Todo)?;

                match raw {
                    #(#result_match_arms),*
                    #fallthrough,
                }
            }
        }

        impl ::ethercrab_wire::EtherCrabWireSized for #name {
            const PACKED_LEN: usize = #size_bytes;

            type Buffer = [u8; #size_bytes];

            fn buffer() -> Self::Buffer {
                [0u8; #size_bytes]
            }
        }

        impl ::ethercrab_wire::EtherCrabWireWriteSized for #name {
            fn pack(&self) -> Self::Buffer {
                let mut buf = [0u8; #size_bytes];

                <Self as ::ethercrab_wire::EtherCrabWireWrite>::pack_to_slice_unchecked(self, &mut buf);

                buf
            }
        }

        #from_primitive_impl
        #into_primitive_impl
    };

    Ok(out)
}
