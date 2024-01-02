use syn::{punctuated::Punctuated, Expr, ExprLit, Ident, Lit, Meta, Token, Type};

pub fn bit_width_attr(attrs: &[syn::Attribute]) -> Result<Option<usize>, syn::Error> {
    usize_attr(attrs, "bits")
}

pub fn usize_attr(attrs: &[syn::Attribute], search: &str) -> Result<Option<usize>, syn::Error> {
    for attr in attrs {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                syn::Meta::Path(_) => (),
                syn::Meta::List(_) => (),
                syn::Meta::NameValue(nv) if nv.path.is_ident(search) => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Int(lit), ..
                    }) = &nv.value
                    {
                        return Ok(Some(lit.base10_parse::<usize>()?));
                    }
                }
                _ => (),
            }
        }
    }

    Ok(None)
}

pub fn attr_exists(attrs: &[syn::Attribute], search: &str) -> Result<bool, syn::Error> {
    for attr in attrs {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                syn::Meta::Path(p) if p.is_ident(search) => return Ok(true),
                _ => (),
            }
        }
    }

    Ok(false)
}

// pub fn field_is_enum_attr(attrs: &[syn::Attribute]) -> Result<bool, syn::Error> {
//     for attr in attrs {
//         let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
//         else {
//             continue;
//         };

//         for meta in nested {
//             match meta {
//                 syn::Meta::Path(_) => (),
//                 syn::Meta::List(_) => (),
//                 syn::Meta::NameValue(nv) if nv.path.is_ident("ty") => {
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
            syn::Meta::List(l) if l.path.is_ident("repr") => {
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
