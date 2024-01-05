use crate::help::{attr_exists, bit_width_attr, usize_attr};
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
    // Will be None for arrays
    pub ty_name: Option<Ident>,
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

    pub skip: bool,
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
            "Only structs with named fields can be derived.",
        ));
    };

    let mut total_field_width = 0;

    let mut field_stuff = Vec::new();

    for field in fields {
        // Unwrap: this is a named-field struct so the field will always have a name.
        let field_name = field.ident.unwrap();
        let field_width = bit_width_attr(&field.attrs)?;

        // Whether to ignore this field when sending AND receiving
        let skip = attr_exists(&field.attrs, "skip")?;

        let pre_skip = usize_attr(&field.attrs, "pre_skip")?.filter(|_| !skip);
        let post_skip = usize_attr(&field.attrs, "post_skip")?.filter(|_| !skip);

        if let Some(skip) = pre_skip {
            total_field_width += skip;
        }

        let bit_start = total_field_width;
        let bit_end = field_width
            .map(|w| total_field_width + w)
            .unwrap_or(total_field_width);
        let byte_start = bit_start / 8;
        let byte_end = bit_end.div_ceil(8);
        let bytes = byte_start..byte_end;
        let bit_offset = bit_start % 8;
        let bits = bit_start..bit_end;

        let ty_name = match field.ty.clone() {
            Type::Path(path) => path.path.get_ident().cloned(),
            _ => None,
        };

        let stuff = FieldStuff {
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

            skip,
        };

        // Validation if we're not skipping this field
        if !skip {
            let Some(field_width) = field_width else {
                return Err(syn::Error::new(
                    stuff.name.span(),
                    "Field must have a width attribute, e.g. #[wire(bits = 4)]",
                ));
            };

            if stuff.bytes.len() > 1 && (bit_offset > 0 || field_width % 8 > 0) {
                return Err(syn::Error::new(
                    stuff.name.span(),
                    format!("Multibyte fields must be byte-aligned at start and end. Current bit position {}", total_field_width),
                ));
            }

            if stuff.bits.len() < 8 && stuff.bytes.len() > 1 {
                return Err(syn::Error::new(
                    stuff.name.span(),
                    "Fields smaller than 8 bits may not cross byte boundaries",
                ));
            }

            total_field_width += field_width;
        }

        if let Some(skip) = post_skip {
            total_field_width += skip;
        }

        field_stuff.push(stuff);
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
