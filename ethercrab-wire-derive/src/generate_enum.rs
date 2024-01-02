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
        quote! {}
    };

    let size_bytes = match repr_type.to_string().as_str() {
        "u8" => 1usize,
        "u16" => 2,
        "u32" => 4,
        invalid => unreachable!("Invalid repr {}", invalid),
    };

    let out = quote! {
        impl ::ethercrab_wire::EtherCatWire for #name {
            const BYTES: usize = #size_bytes;

            type Arr = [u8; #size_bytes];

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                // TODO: If only one byte, just write it to `buf[0]`
                let mut buf = &mut buf[0..Self::BYTES];

                #pack

                buf
            }

            fn pack(&self) -> Self::Arr {
                // TODO: Optimise if only one byte in length

                let mut buf = [0u8; #size_bytes];

                self.pack_to_slice_unchecked(&mut buf);

                buf
            }

            fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab_wire::WireError> {
                // TODO: If only one byte, just get it from `buf[0]`
                let raw = buf.get(0..Self::BYTES).map(|bytes| {
                    #repr_type::from_le_bytes(bytes.try_into().unwrap())
                }).ok_or(::ethercrab_wire::WireError::Todo)?;

                match raw {
                    #(#result_match_arms),*
                    #fallthrough,
                }
            }
        }

        #from_primitive_impl
        #into_primitive_impl
    };

    Ok(out)
}
