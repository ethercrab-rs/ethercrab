use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::ParseStream, parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(EtherCatWire)]
pub fn ethercat_wire(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Enum(d) => todo!(),
        Data::Struct(s) => todo!(),
        Data::Union(_) => {
            return syn::Error::new(input.ident.span(), "Unions are not supported")
                .to_compile_error()
                .into()
        }
    }

    // Build the output, possibly using quasi-quotation
    let expanded = quote! {
        // ...
    };

    // Hand the output tokens back to the compiler
    TokenStream::from(expanded)
}

#[cfg(test)]
mod tests {
    #[test]
    fn trybuild_cases() {
        let t = trybuild::TestCases::new();

        t.compile_fail("ui/*.rs");
    }
}
