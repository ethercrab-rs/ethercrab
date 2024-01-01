use crate::help::{bit_width_attr, enum_repr_ty};
use syn::{punctuated::Punctuated, DataEnum, DeriveInput, Expr, ExprLit, Lit, Meta, Token, Type};

pub struct EnumStuff {
    /// Width in bits on the wire.
    pub width: usize,
    pub repr_type: Type,
}

pub fn parse_enum(
    e: DataEnum,
    DeriveInput { attrs, ident, .. }: DeriveInput,
) -> syn::Result<EnumStuff> {
    let width = bit_width_attr(&attrs)?;

    let repr = enum_repr_ty(&attrs, &ident)?;

    let Some(width) = width else {
        return Err(syn::Error::new(
            ident.span(),
            "Enum bit width is required, e.g. #[wire(bits = 8)]",
        ));
    };

    Ok(EnumStuff {
        width,
        repr_type: repr,
    })
}
