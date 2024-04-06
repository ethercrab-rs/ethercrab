use crate::help::{
    all_valid_attrs, attr_exists, enum_repr_ty, variant_alternatives, variant_is_default,
};
use syn::{DataEnum, DeriveInput, Expr, ExprLit, ExprUnary, Ident, Lit, UnOp};

#[derive(Clone)]
pub struct EnumMeta {
    /// Width in bits on the wire.
    // pub width: usize,
    pub repr_type: Ident,

    pub variants: Vec<VariantMeta>,

    pub catch_all: Option<VariantMeta>,
    pub default_variant: Option<VariantMeta>,
}

#[derive(Clone)]
pub struct VariantMeta {
    pub name: Ident,
    pub discriminant: i128,
    pub catch_all: bool,
    pub default: bool,
    pub alternatives: Vec<i128>,
}

pub fn parse_enum(
    e: DataEnum,
    DeriveInput { attrs, ident, .. }: DeriveInput,
) -> syn::Result<EnumMeta> {
    // let width = bit_width_attr(&attrs)?;

    all_valid_attrs(&attrs, &["bits", "bytes"])?;

    let repr = enum_repr_ty(&attrs, &ident)?;

    if ["isize", "usize"].iter().any(|bad| repr == bad) {
        return Err(syn::Error::new(
            repr.span(),
            "usize and isize may not be used as enum repr as these types can change size based on target platform. Use an i* or u* type instead.".to_string(),
        ));
    }

    // --- Variants

    let mut discriminant_accum = 0;
    let mut variants = Vec::new();
    let mut catch_all = None;
    let mut default_variant = None;

    for variant in e.variants {
        all_valid_attrs(&variant.attrs, &["alternatives", "catch_all"])?;

        let ident = variant.ident;

        let variant_discriminant = match variant.discriminant {
            Some((
                _,
                Expr::Lit(ExprLit {
                    lit: Lit::Int(discr),
                    ..
                }),
            )) => {
                // Parse to i128 to fit any possible value we could encounter
                discr.base10_parse::<i128>()?
            }
            Some((
                _,
                Expr::Unary(ExprUnary {
                    expr,
                    op: UnOp::Neg(_),
                    ..
                }),
            )) => {
                match *expr {
                    Expr::Lit(ExprLit {
                        lit: Lit::Int(discr),
                        ..
                    }) => {
                        discr
                            // Parse to i128 to fit any possible value we could encounter
                            .base10_parse::<i128>()
                            // Negate value because we matched on `UnOp::Neg` above.
                            .map(|value| -value)?
                    }
                    _ => return Err(syn::Error::new(repr.span(), "Invalid discriminant format")),
                }
            }
            None => discriminant_accum + 1,
            _ => return Err(syn::Error::new(repr.span(), "Invalid discriminant format")),
        };

        let is_default = variant_is_default(&variant.attrs);
        let is_catch_all = attr_exists(&variant.attrs, "catch_all");

        let alternatives = variant_alternatives(&variant.attrs)?;

        if is_catch_all && !alternatives.is_empty() {
            return Err(syn::Error::new(
                ident.span(),
                "Catch all cannot have alternatives",
            ));
        }

        let record = VariantMeta {
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
            let alt = VariantMeta {
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

    Ok(EnumMeta {
        // width,
        repr_type: repr,
        variants,
        catch_all,
        default_variant,
    })
}
