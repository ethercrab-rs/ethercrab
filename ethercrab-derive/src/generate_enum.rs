use crate::parse_enum::EnumStuff;
use quote::quote;
use syn::DeriveInput;

pub fn generate_enum(
    parsed: EnumStuff,
    input: &DeriveInput,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = input.ident.clone();
    let repr_type = parsed.repr_type;

    let out = quote! {
        impl ::ethercrab::derive::WireFieldEnum for #name {
            const BYTES: usize = #repr_type::BITS as usize / 8;

            type Repr = #repr_type;

            fn unpack_to_repr(buf: &[u8]) -> Result<Self::Repr, ::ethercrab::error::Error> {
                let chunk = buf.get(0..Self::BYTES).ok_or(::ethercrab::error::Error::Internal)?;

                Ok(Self::Repr::from_le_bytes(chunk.try_into().unwrap()))
            }
        }
    };

    Ok(out)
}
