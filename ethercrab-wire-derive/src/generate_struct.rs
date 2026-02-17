use crate::parse_struct::StructMeta;
use proc_macro2::{Ident, Span};
use quote::quote;
use std::str::FromStr;
use syn::DeriveInput;

pub fn generate_struct_write(parsed: &StructMeta, input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = input.ident.clone();
    let size_bytes = parsed.width_bits.div_ceil(8);

    let fields_pack = parsed.fields.clone().into_iter().map(|field| {
        let name = field.name;
        let field_ty = field.ty;
        let byte_start = field.bytes.start;
        let bit_start = field.bit_offset;

        if field.skip {
            return quote! {};
        }

        let ty_name = field
            .ty_name
            .unwrap_or_else(|| Ident::new("UnknownTypeStopLookingAtMe", Span::call_site()));

        let field_access = if parsed.repr_packed {
            quote! {{
                unsafe { core::ptr::read_unaligned(&raw const self.#name) }
            }}
        } else {
            quote! {self.#name}
        };

        // Small optimisation
        if ty_name == "u8" || ty_name == "bool" {
            let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
            let mask = proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

            quote! {
                buf[#byte_start] |= ((#field_access as u8) << #bit_start) & #mask;
            }
        }
        // Single byte fields need merging into the other data
        else if field.bytes.len() == 1 {
            let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
            let mask = proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

            quote! {
                let mut field_buf = [0u8; 1];
                let res = <#field_ty as ::ethercrab_wire::EtherCrabWireWrite>::pack_to_slice_unchecked(&#field_access, &mut field_buf)[0];

                buf[#byte_start] |= (res << #bit_start) & #mask;
            }
        }
        // Assumption: multi-byte fields are byte-aligned. This should be validated during parse.
        else {
            let byte_end = field.bytes.end;
            quote! {
                <#field_ty as ::ethercrab_wire::EtherCrabWireWrite>::pack_to_slice_unchecked(&#field_access, &mut buf[#byte_start..#byte_end]);
            }
        }
    });

    let (impl_generics, type_generics, where_clause) = parsed.generics.split_for_impl();
    quote! {
        impl #impl_generics ::ethercrab_wire::EtherCrabWireWrite for #name #type_generics
        #where_clause {
            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let buf = match buf.get_mut(0..#size_bytes) {
                    Some(buf) => buf,
                    None => unreachable!()
                };

                unsafe {
                    buf.as_mut_ptr().write_bytes(0u8, buf.len());
                }

                #(#fields_pack)*

                buf
            }

            fn packed_len(&self) -> usize {
                #size_bytes
            }
        }

        impl #impl_generics ::ethercrab_wire::EtherCrabWireWriteSized for #name #type_generics
        #where_clause {
            fn pack(&self) -> Self::Buffer {
                let mut buf = [0u8; #size_bytes];

                <Self as ::ethercrab_wire::EtherCrabWireWrite>::pack_to_slice_unchecked(self, &mut buf);

                buf
            }
        }
    }
}

pub fn generate_struct_read(parsed: &StructMeta, input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = input.ident.clone();
    let size_bytes = parsed.width_bits.div_ceil(8);

    let fields_unpack = parsed.fields.clone().into_iter().map(|field| {
        let ty = field.ty;
        let name = field.name;
        let byte_start = field.bytes.start;
        let bit_start = field.bit_offset;
        let ty_name = field
            .ty_name
            .unwrap_or_else(|| Ident::new("UnknownTypeStopLookingAtMe", Span::call_site()));

        if field.skip {
            return quote! {
                #name: Default::default()
            }
        }

        if field.bits.len() <= 8 {
            let mask = (2u16.pow(field.bits.len() as u32) - 1) << bit_start;
            let mask =
                proc_macro2::TokenStream::from_str(&format!("{:#010b}", mask)).unwrap();

            if ty_name == "bool" {
                quote! {
                    #name: ((buf.get(#byte_start).ok_or(::ethercrab_wire::WireError::ReadBufferTooShort)? & #mask) >> #bit_start) > 0
                }
            }
            // Small optimisation
            else if ty_name == "u8" {
                quote! {
                    #name: (buf.get(#byte_start).ok_or(::ethercrab_wire::WireError::ReadBufferTooShort)? & #mask) >> #bit_start
                }
            }
            // Anything else will be a struct or an enum
            else {
                quote! {
                    #name: {
                        let masked = (buf.get(#byte_start).ok_or(::ethercrab_wire::WireError::ReadBufferTooShort)? & #mask) >> #bit_start;

                        <#ty as ::ethercrab_wire::EtherCrabWireRead>::unpack_from_slice(&[masked])?
                    }
                }
            }
        }
        // Assumption: multi-byte fields are byte-aligned. This must be validated during parse.
        else {
            let start_byte = field.bytes.start;
            let end_byte = field.bytes.end;

            quote! {
                #name: <#ty as ::ethercrab_wire::EtherCrabWireRead>::unpack_from_slice(buf.get(#start_byte..#end_byte).ok_or(::ethercrab_wire::WireError::ReadBufferTooShort)?)?
            }
        }
    });

    let (impl_generics, type_generics, where_clause) = parsed.generics.split_for_impl();
    quote! {
        impl #impl_generics ::ethercrab_wire::EtherCrabWireRead for #name #type_generics
        #where_clause {
            fn unpack_from_slice(buf: &[u8]) -> Result<Self, ::ethercrab_wire::WireError> {
                let buf = buf.get(0..#size_bytes).ok_or(::ethercrab_wire::WireError::ReadBufferTooShort)?;

                Ok(Self {
                    #(#fields_unpack),*
                })
            }
        }
    }
}

pub fn generate_sized_impl(parsed: &StructMeta, input: &DeriveInput) -> proc_macro2::TokenStream {
    let name = input.ident.clone();
    let size_bytes = parsed.width_bits.div_ceil(8);

    let (impl_generics, type_generics, where_clause) = parsed.generics.split_for_impl();
    quote! {
        impl #impl_generics ::ethercrab_wire::EtherCrabWireSized for #name #type_generics
        #where_clause {
            const PACKED_LEN: usize = #size_bytes;

            type Buffer = [u8; #size_bytes];

            fn buffer() -> Self::Buffer {
                [0u8; #size_bytes]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireWrite};

    #[test]
    fn generic_struct() {
        #[derive(EtherCrabWireReadWrite, PartialEq, Debug)]
        #[wire(bytes = 8)]
        struct TestTypeGeneric<T: EtherCrabWireReadWrite> {
            #[wire(bits = 32)]
            a: i32,
            #[wire(bits = 32)]
            b: T,
        }
        let test_type_generic = TestTypeGeneric::<u32> {
            a: -16,
            b: u32::MAX,
        };
        let mut slice = [0u8; 8];
        test_type_generic.pack_to_slice(&mut slice).unwrap();
        assert_eq!(
            Ok(test_type_generic),
            TestTypeGeneric::<u32>::unpack_from_slice(&slice)
        );

        #[derive(EtherCrabWireReadWrite, PartialEq, Debug)]
        #[wire(bytes = 8)]
        struct TestWhereClause<T>
        where
            T: EtherCrabWireReadWrite,
        {
            #[wire(bits = 32)]
            a: i32,
            #[wire(bits = 32)]
            b: T,
        }
        let test_where_clause = TestWhereClause::<u32> {
            a: -16,
            b: u32::MAX,
        };
        let mut slice = [0u8; 8];
        test_where_clause.pack_to_slice(&mut slice).unwrap();
        assert_eq!(
            Ok(test_where_clause),
            TestWhereClause::<u32>::unpack_from_slice(&slice)
        );
    }
}
