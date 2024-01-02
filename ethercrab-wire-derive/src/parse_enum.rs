use crate::help::{attr_exists, enum_repr_ty, variant_alternatives, variant_is_default};
use syn::{DataEnum, DeriveInput, Expr, ExprLit, Ident, Lit};

// TODO: Rename all these `*Stuff` fields lol
pub struct EnumStuff {
    /// Width in bits on the wire.
    // pub width: usize,
    pub repr_type: Ident,

    pub variants: Vec<VariantStuff>,

    pub catch_all: Option<VariantStuff>,
    pub default_variant: Option<VariantStuff>,
}

#[derive(Clone)]
pub struct VariantStuff {
    pub name: Ident,
    pub discriminant: u32,
    pub catch_all: bool,
    pub default: bool,
    pub alternatives: Vec<u32>,
}

pub fn parse_enum(
    e: DataEnum,
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

    // --- Variants

    let mut discriminant_accum = 0;
    let mut variants = Vec::new();
    let mut catch_all = None;
    let mut default_variant = None;

    for variant in e.variants {
        let ident = variant.ident;

        let variant_discriminant = match variant.discriminant {
            Some((
                _,
                Expr::Lit(ExprLit {
                    lit: Lit::Int(discr),
                    ..
                }),
            )) => discr.base10_parse::<u32>()?,
            None => discriminant_accum + 1,
            _ => return Err(syn::Error::new(repr.span(), "Invalid discriminant format")),
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

        let record = VariantStuff {
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
            let alt = VariantStuff {
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

    Ok(EnumStuff {
        // width,
        repr_type: repr,
        variants,
        catch_all,
        default_variant,
    })
}
