//! # Experimental
//!
//! This crate may expand in the future but is currently only used internally by
//! [`ethercrab`](https://crates.io/crates/ethercrab) itself. It is experimental and may change at
//! any time, so please do not depend on or rely on any of this crate's items.

#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]

mod generate_enum;
mod generate_struct;
mod help;
mod parse_enum;
mod parse_struct;

use generate_enum::{generate_enum_read, generate_enum_write};
use generate_struct::{generate_struct_read, generate_struct_write};
use parse_enum::parse_enum;
use parse_struct::parse_struct;
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput};

/// Items that can be written to/read from the wire.
#[proc_macro_derive(EtherCrabWireReadWrite, attributes(wire))]
pub fn ether_crab_wire(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let res = match input.clone().data {
        Data::Enum(e) => parse_enum(e, input.clone()).and_then(|parsed| {
            let mut tokens = generate_enum_write(parsed.clone(), &input)?;

            tokens.extend(generate_enum_read(parsed, &input)?);

            Ok(tokens)
        }),
        Data::Struct(s) => parse_struct(s, input.clone()).and_then(|parsed| {
            let mut tokens = generate_struct_write(parsed.clone(), &input)?;

            tokens.extend(generate_struct_read(parsed, &input)?);

            Ok(tokens)
        }),
        Data::Union(_) => Err(syn::Error::new(
            input.ident.span(),
            "Unions are not supported",
        )),
    };

    let res = match res {
        Ok(res) => res,
        Err(e) => return e.to_compile_error().into(),
    };

    TokenStream::from(res)
}

/// Items that can only be read from the wire.
#[proc_macro_derive(EtherCrabWireRead, attributes(wire))]
pub fn ether_crab_wire_readonly(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let res = match input.clone().data {
        Data::Enum(e) => {
            parse_enum(e, input.clone()).and_then(|parsed| generate_enum_read(parsed, &input))
        }
        Data::Struct(s) => {
            parse_struct(s, input.clone()).and_then(|parsed| generate_struct_read(parsed, &input))
        }
        Data::Union(_) => Err(syn::Error::new(
            input.ident.span(),
            "Unions are not supported",
        )),
    };

    let res = match res {
        Ok(res) => res,
        Err(e) => return e.to_compile_error().into(),
    };

    TokenStream::from(res)
}

#[cfg(test)]
mod tests {
    #[test]
    fn trybuild_cases() {
        let t = trybuild::TestCases::new();

        t.compile_fail("ui/*.rs");
    }
}
