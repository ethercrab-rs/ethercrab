mod generate_enum;
mod generate_struct;
mod help;
mod parse_enum;
mod parse_struct;

use generate_enum::generate_enum;
use generate_struct::generate_struct;
use parse_enum::parse_enum;
use parse_struct::parse_struct;
use proc_macro::TokenStream;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(EtherCatWire, attributes(wire))]
pub fn ethercat_wire(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let res = match input.clone().data {
        Data::Enum(e) => {
            parse_enum(e, input.clone()).and_then(|parsed| generate_enum(parsed, &input))
        }
        Data::Struct(s) => {
            parse_struct(s, input.clone()).and_then(|parsed| generate_struct(parsed, &input))
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
