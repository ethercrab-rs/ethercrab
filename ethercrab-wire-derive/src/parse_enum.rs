use crate::{
    help::{
        all_valid_attrs, attr_exists, enum_repr_ty, variant_alternatives, variant_is_default,
        STRUCT_ATTRS,
    },
    parse_struct::{parse_struct_fields, StructMeta},
};
use syn::{Attribute, DataEnum, DeriveInput, Expr, ExprLit, FieldsNamed, Ident, Lit};

#[derive(Clone)]
pub enum EnumMeta {
    /// Enums that are representable as a `u*` with no named fields.
    Simple(EnumMetaSimple),
    /// An enum with named fields.
    Named(EnumMetaNamed),
}

#[derive(Clone)]
pub struct EnumMetaSimple {
    /// Width in bits on the wire.
    // pub width: usize,
    pub repr_type: Ident,
    pub variants: Vec<SimpleVariantMeta>,
    pub catch_all: Option<SimpleVariantMeta>,
    pub default_variant: Option<SimpleVariantMeta>,
}

#[derive(Clone)]
pub struct EnumMetaNamed {
    pub variants: Vec<NamedVariantMeta>,
}

#[derive(Clone)]
pub struct SimpleVariantMeta {
    pub name: Ident,
    pub discriminant: u32,
    pub catch_all: bool,
    pub default: bool,
    pub alternatives: Vec<u32>,
}

#[derive(Clone)]
pub struct NamedVariantMeta {
    pub name: Ident,
    pub fields: StructMeta,
}

pub fn parse_enum(
    e: DataEnum,
    DeriveInput {
        attrs,
        ident: enum_ident,
        ..
    }: DeriveInput,
) -> syn::Result<EnumMeta> {
    // let width = bit_width_attr(&attrs)?;

    let has_named_fields = e
        .variants
        .iter()
        .any(|variant| matches!(variant.fields, syn::Fields::Named(..)));

    if !has_named_fields {
        parse_unnamed_fields_enum(&attrs, &enum_ident, &e.variants)
    } else {
        todo!()
    }
}

fn parse_unnamed_fields_enum(
    attrs: &[Attribute],
    enum_ident: &Ident,
    parsed_variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
) -> Result<EnumMeta, syn::Error> {
    all_valid_attrs(&attrs, &["bits", "bytes"])?;

    let repr = enum_repr_ty(&attrs, enum_ident)?;

    // let Some(width) = width else {
    //     return Err(syn::Error::new(
    //         ident.span(),
    //         "Enum bit width is required, e.g. #[wire(bits = 8)]",
    //     ));
    // };

    let allowed_reprs = ["u8", "u16", "u32"];

    let valid = allowed_reprs.iter().any(|allow| repr == allow);

    if !valid {
        return Err(syn::Error::new(
            repr.span(),
            format!("Allowed reprs are {}", allowed_reprs.join(", ")),
        ));
    }

    // --- Variants

    let mut discriminant_accum = 0;
    let mut variants = Vec::new();
    let mut catch_all = None;
    let mut default_variant = None;

    for variant in parsed_variants {
        match variant.fields {
            syn::Fields::Named(FieldsNamed { .. }) => {
                unreachable!("Mixed unnamed and named fields!");
            }
            syn::Fields::Unnamed(_) => {
                all_valid_attrs(&variant.attrs, &["catch_all"])?;
            }
            syn::Fields::Unit => {
                all_valid_attrs(&variant.attrs, &["alternatives"])?;
            }
        }

        let ident = variant.ident.clone();

        let variant_discriminant = match variant.discriminant.clone() {
            Some((
                _,
                Expr::Lit(ExprLit {
                    lit: Lit::Int(discr),
                    ..
                }),
            )) => discr.base10_parse::<u32>()?,
            None => discriminant_accum + 1,
            _ => {
                return Err(syn::Error::new(
                    ident.span(),
                    "Invalid discriminant format, must be a literal like 0xaa or 10u8",
                ))
            }
        };

        let is_default = variant_is_default(&variant.attrs)?;
        let is_catch_all = attr_exists(&variant.attrs, "catch_all")?;

        let alternatives = variant_alternatives(&variant.attrs)?;

        if is_catch_all && !alternatives.is_empty() {
            return Err(syn::Error::new(
                ident.span(),
                "Catch all cannot have alternatives",
            ));
        }

        let record = SimpleVariantMeta {
            name: ident.clone(),
            discriminant: variant_discriminant,
            catch_all: is_catch_all,
            alternatives: alternatives.clone(),
            default: is_default,
        };

        if is_catch_all {
            let old = catch_all.replace(record.clone());

            if old.is_some() {
                return Err(syn::Error::new(
                    ident.span(),
                    "Only one catch all variant is allowed",
                ));
            }
        }

        if is_default {
            let old = default_variant.replace(record.clone());

            if old.is_some() {
                return Err(syn::Error::new(
                    ident.span(),
                    "Only one default variant is allowed",
                ));
            }
        }

        discriminant_accum = variant_discriminant;

        variants.push(record.clone());

        for alternative in alternatives {
            let alt = SimpleVariantMeta {
                name: ident.clone(),
                discriminant: alternative,
                alternatives: Vec::new(),
                default: false,
                catch_all: false,
            };

            variants.push(alt);

            discriminant_accum = alternative;
        }
    }

    Ok(EnumMeta::Simple(EnumMetaSimple {
        // width,
        repr_type: repr,
        variants,
        catch_all,
        default_variant,
    }))
}

fn parse_named_fields(
    attrs: &[Attribute],
    _enum_ident: &Ident,
    parsed_variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
) -> Result<EnumMeta, syn::Error> {
    let mut variants = Vec::new();

    for variant in parsed_variants {
        let syn::Fields::Named(FieldsNamed { named: fields, .. }) = variant.fields.clone() else {
            return Err(syn::Error::new(variant.ident.span(), "Named fields only"));
        };

        all_valid_attrs(&attrs, STRUCT_ATTRS)?;

        let fields = parse_struct_fields(&variant.attrs, variant.ident.clone(), &fields)?;

        variants.push(NamedVariantMeta {
            name: variant.ident.clone(),
            fields,
        });
    }

    Ok(EnumMeta::Named(EnumMetaNamed { variants }))
}
