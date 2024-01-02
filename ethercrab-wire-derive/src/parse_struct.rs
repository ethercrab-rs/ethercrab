use crate::help::{bit_width_attr, usize_attr};
use std::ops::Range;
use syn::{DataStruct, DeriveInput, Fields, FieldsNamed, Ident, Type, Visibility};

pub struct StructStuff {
    /// Width in bits on the wire.
    pub width: usize,

    pub fields: Vec<FieldStuff>,
}

#[derive(Clone)]
pub struct FieldStuff {
    pub vis: Visibility,
    pub name: Ident,
    pub ty: Type,
    pub ty_name: Ident,
    pub bit_start: usize,
    pub bit_end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    /// Offset of the starting bit in the starting byte.
    pub bit_offset: usize,

    pub bits: Range<usize>,
    pub bytes: Range<usize>,

    pub pre_skip: Option<usize>,
    pub post_skip: Option<usize>,
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

        let pre_skip = usize_attr(&field.attrs, "pre_skip")?;
        let post_skip = usize_attr(&field.attrs, "post_skip")?;

        if let Some(skip) = pre_skip {
            total_field_width += skip;
        }

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
        let bytes = byte_start..byte_end;
        let bit_offset = bit_start % 8;
        let bits = bit_start..bit_end;

        if bytes.len() > 1 && (bit_offset > 0 || field_width % 8 > 0) {
            return Err(syn::Error::new(
                field_name.span(),
                "Multibyte fields must be byte-aligned at start and end",
            ));
        }

        if bits.len() < 8 && bytes.len() > 1 {
            return Err(syn::Error::new(
                field_name.span(),
                "Fields smaller than 8 bits may not cross byte boundaries",
            ));
        }

        let ty_name = match field.ty.clone() {
            Type::Path(path) => path
                .path
                .get_ident()
                .cloned()
                .ok_or(syn::Error::new(field_name.span(), "Type is required")),
            _ => Err(syn::Error::new(field_name.span(), "Invalid type name")),
        }?
        .clone();

        if let Some(skip) = post_skip {
            total_field_width += skip;
        }

        field_stuff.push(FieldStuff {
            name: field_name,
            vis: field.vis,
            ty: field.ty,
            ty_name,

            bits,
            bytes,

            bit_start,
            bit_end,
            byte_start,
            byte_end,

            bit_offset,

            pre_skip,
            post_skip,
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

    Ok(StructStuff {
        width,
        fields: field_stuff,
    })
}
