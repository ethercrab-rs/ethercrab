use proc_macro2::Span;
use std::collections::HashSet;
use syn::{
    punctuated::Punctuated, spanned::Spanned, Expr, ExprArray, ExprLit, Ident, Lit, Meta, Token,
    Type,
};

pub const MY_ATTRIBUTE: &str = "wire";

fn my_attributes(attrs: &[syn::Attribute]) -> impl Iterator<Item = &syn::Attribute> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident(MY_ATTRIBUTE))
}

pub fn bit_width_attr(attrs: &[syn::Attribute]) -> Result<Option<usize>, syn::Error> {
    let bits = usize_attr(attrs, "bits")?;
    let bytes = usize_attr(attrs, "bytes")?.map(|bytes| bytes * 8);

    if bits.is_some() && bytes.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "'bits' and 'bytes' attribute not allowed at the same time",
        ));
    }

    Ok(bits.or(bytes))
}

pub fn usize_attr(attrs: &[syn::Attribute], search: &str) -> Result<Option<usize>, syn::Error> {
    for attr in my_attributes(attrs) {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                Meta::Path(_) | Meta::List(_) => (),
                Meta::NameValue(nv) if nv.path.is_ident(search) => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Int(lit), ..
                    }) = &nv.value
                    {
                        return Ok(Some(lit.base10_parse::<usize>()?));
                    }
                }
                Meta::NameValue(_) => (),
            }
        }
    }

    Ok(None)
}

/// Check that all attributes are supported
pub fn all_valid_attrs(attrs: &[syn::Attribute], allowed: &[&str]) -> Result<(), syn::Error> {
    let allowed = allowed
        .iter()
        .map(|s| Ident::new(s, Span::call_site()))
        .collect::<HashSet<_>>();

    let mut idents = HashSet::new();

    for attr in my_attributes(attrs) {
        // Skip other attributes like doc comments etc
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            let ident = match meta {
                Meta::Path(p) => p.get_ident().cloned().expect("Path identifier required"),
                Meta::List(_) => unreachable!("Unsupported"),
                Meta::NameValue(nv) => nv
                    .path
                    .get_ident()
                    .cloned()
                    .expect("NameValue identifier required"),
            };

            let None = idents.replace(ident.clone()) else {
                panic!("Duplicate attribute found {}", ident);
            };
        }
    }

    let mut bad = idents.difference(&allowed);

    if let Some(first) = bad.next() {
        return Err(syn::Error::new(first.span(), "Invalid attribute"));
    }

    Ok(())
}

pub fn attr_exists(attrs: &[syn::Attribute], search: &str) -> bool {
    for attr in my_attributes(attrs) {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                Meta::Path(p) if p.is_ident(search) => return true,
                _ => (),
            }
        }
    }

    false
}

// pub fn field_is_enum_attr(attrs: &[syn::Attribute]) -> Result<bool, syn::Error> {
//     for attr in my_attributes(attrs) {
//         let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
//         else {
//             continue;
//         };

//         for meta in nested {
//             match meta {
//                 Meta::Path(_) => (),
//                 Meta::List(_) => (),
//                 Meta::NameValue(nv) if nv.path.is_ident("ty") => {
//                     if let Expr::Lit(ExprLit {
//                         lit: Lit::Str(s), ..
//                     }) = &nv.value
//                     {
//                         return Ok(s.value() == "enum");
//                     }
//                 }
//                 _ => (),
//             }
//         }
//     }

//     Ok(false)
// }

pub fn enum_repr_ty(attrs: &[syn::Attribute], ident: &Ident) -> Result<Ident, syn::Error> {
    for attr in attrs {
        match attr.meta.clone() {
            Meta::List(l) if l.path.is_ident("repr") => {
                let ty = l.parse_args::<Type>()?;

                if let Type::Path(ty) = ty {
                    return ty
                        .path
                        .get_ident()
                        .cloned()
                        .ok_or_else(|| syn::Error::new(ident.span(), "Repr is not a valid type"));
                }
            }
            _ => (),
        }
    }

    Err(syn::Error::new(
        ident.span(),
        "Enums must have a #[repr()] attribute",
    ))
}

/// Look for `alternatives = [1,2,3]` attribute on enum variant.
pub fn variant_alternatives(attrs: &[syn::Attribute]) -> Result<Vec<i128>, syn::Error> {
    for attr in my_attributes(attrs) {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                Meta::Path(_) | Meta::List(_) => (),
                Meta::NameValue(nv) if nv.path.is_ident("alternatives") => {
                    if let Expr::Array(ExprArray { elems, .. }) = &nv.value {
                        return elems
                            .iter()
                            .map(|elem| {
                                let Expr::Lit(ExprLit {
                                    lit: Lit::Int(lit), ..
                                }) = elem.clone()
                                else {
                                    return Err(syn::Error::new(
                                        elem.span(),
                                        "Alternatives must be numbers",
                                    ));
                                };

                                lit.base10_parse::<i128>()
                            })
                            .collect::<Result<Vec<_>, _>>();
                    }
                }
                Meta::NameValue(_) => (),
            }
        }
    }

    Ok(Vec::new())
}

pub fn variant_is_default(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        match attr.meta {
            Meta::Path(ref p) if p.is_ident("default") => return true,
            _ => continue,
        }
    }

    false
}
