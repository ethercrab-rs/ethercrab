use crate::help::bit_width_attr;
use std::ops::Range;
use syn::{
    punctuated::Punctuated, DataEnum, DataStruct, DeriveInput, Expr, ExprLit, Fields, FieldsNamed,
    Ident, Lit, Meta, Token, Type, Visibility,
};

#[derive(Debug)]
pub struct StructStuff {
    /// Width in bits on the wire.
    pub width: usize,

    pub fields: Vec<FieldStuff>,
}

#[derive(Debug)]
pub struct FieldStuff {
    vis: Visibility,
    name: Ident,
    ty: Type,
    bit_start: usize,
    bit_end: usize,
    byte_start: usize,
    byte_end: usize,
    /// Offset of the starting bit in the starting byte.
    bit_offset: usize,

    bits: Range<usize>,
    bytes: Range<usize>,
}

pub fn parse_struct(
    s: DataStruct,
    DeriveInput { attrs, ident, .. }: DeriveInput,
) -> syn::Result<StructStuff> {
    // --- Struct attributes

    let width = bit_width_attr(&attrs)?;

    let Some(width) = width else {
        return Err(syn::Error::new(
            ident.span(),
            "Struct total bit width is required, e.g. #[wire(bits = 32)]",
        ));
    };

    // --- Fields

    let Fields::Named(FieldsNamed { named: fields, .. }) = s.fields else {
        return Err(syn::Error::new(
            ident.span(),
            "Only structs with named fields are supported.",
        ));
    };

    let mut total_field_width = 0;

    let mut field_stuff = Vec::new();

    for field in fields {
        // Unwrap: this is a named-field struct so the field will always have a name.
        let field_name = field.ident.unwrap();
        let field_width = bit_width_attr(&field.attrs)?;

        let Some(field_width) = field_width else {
            return Err(syn::Error::new(
                field_name.span(),
                "Field must have a width attribute, e.g. #[wire(bits = 4)]",
            ));
        };

        // TODO: Check bit lengths actually fit in the given type

        let bit_start = total_field_width;
        let bit_end = total_field_width + field_width;
        let byte_start = bit_start / 8;
        let byte_end = bit_end.div_ceil(8);

        field_stuff.push(FieldStuff {
            name: field_name,
            vis: field.vis,
            ty: field.ty,

            bits: bit_start..bit_end,
            bytes: byte_start..byte_end,

            bit_start,
            bit_end,
            byte_start,
            byte_end,

            bit_offset: bit_start / 8,
        });

        total_field_width += field_width;
    }

    if total_field_width != width {
        return Err(syn::Error::new(
            ident.span(),
            format!(
                "Total field width is {}, expected {} from struct definition",
                total_field_width, width
            ),
        ));
    }

    Ok(dbg!(StructStuff {
        width,
        fields: field_stuff,
    }))
}
