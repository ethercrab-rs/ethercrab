use crate::help::{bit_width_attr, enum_repr_ty};
use syn::{DataEnum, DeriveInput, Ident};

pub struct EnumStuff {
    /// Width in bits on the wire.
    // pub width: usize,
    pub repr_type: Ident,
}

pub fn parse_enum(
    _e: DataEnum,
    DeriveInput { attrs, ident, .. }: DeriveInput,
) -> syn::Result<EnumStuff> {
    // let width = bit_width_attr(&attrs)?;

    let repr = enum_repr_ty(&attrs, &ident)?;

    let allowed = ["u8", "u16", "u32"];

    let valid = allowed.iter().any(|allow| repr == allow);

    if !valid {
        return Err(syn::Error::new(
            repr.span(),
            format!("Allowed reprs are {}", allowed.join(", ")),
        ));
    }

    // let Some(width) = width else {
    //     return Err(syn::Error::new(
    //         ident.span(),
    //         "Enum bit width is required, e.g. #[wire(bits = 8)]",
    //     ));
    // };

    Ok(EnumStuff {
        // width,
        repr_type: repr,
    })
}
