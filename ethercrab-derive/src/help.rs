use syn::{punctuated::Punctuated, Expr, ExprLit, Ident, Lit, Meta, Token, Type};

pub fn bit_width_attr(attrs: &[syn::Attribute]) -> Result<Option<usize>, syn::Error> {
    let mut width = None;

    for attr in attrs {
        let Ok(nested) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match meta {
                syn::Meta::Path(_) => (),
                syn::Meta::List(_) => (),
                syn::Meta::NameValue(nv) if nv.path.is_ident("bits") => {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Int(lit), ..
                    }) = &nv.value
                    {
                        width = Some(lit.base10_parse::<usize>()?);
                    }
                }
                syn::Meta::NameValue(nv) => {
                    dbg!("Ignore attribute {:?}", nv.path.get_ident());
                }
            }
        }
    }

    Ok(width)
}

pub fn enum_repr_ty(attrs: &[syn::Attribute], ident: &Ident) -> Result<Type, syn::Error> {
    for attr in attrs {
        match attr.meta.clone() {
            syn::Meta::List(l) if l.path.is_ident("repr") => return l.parse_args::<Type>(),
            _ => (),
        }
    }

    Err(syn::Error::new(
        ident.span(),
        "Enums must have a #[repr()] attribute",
    ))
}
