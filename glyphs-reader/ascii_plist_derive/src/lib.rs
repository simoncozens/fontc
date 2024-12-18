//! A simple and limited proc macro for parsing ASCII plists.
//!
//! This tool was written for and is tailored to the case of parsing the .glyphs
//! files generated by [Glyphs.app].
//!
//! [Glyphs.app]: https://glyphsapp.com

extern crate proc_macro;

use attrs::FieldAttrs;
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use std::iter;
use syn::{parse_macro_input, spanned::Spanned, Data, DeriveInput, Field, Fields, Path, Type};

mod attrs;

#[proc_macro_derive(FromPlist, attributes(fromplist))]
pub fn from_plist(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let field_cases = match add_fieldcases(&input) {
        Ok(thing) => thing,
        Err(e) => return e.into_compile_error().into(),
    };
    let name = input.ident;

    let expanded = quote! {
        impl FromPlist for #name {
            fn parse(tokenizer: &mut Tokenizer<'_>) -> Result<Self, crate::plist::Error> {
                use crate::plist::Error;

                tokenizer.eat(b'{')?;
                let mut rec = #name::default();
                loop {
                    if tokenizer.eat(b'}').is_ok() {
                        return Ok((rec));
                    }
                    let key = tokenizer.lex()?;
                    tokenizer.eat(b'=')?;
                    match key.as_str() {
                        #field_cases
                        Some(unrecognized) => tokenizer.skip_rec()?,
                        _ => (),
                    };
                    tokenizer.eat(b';')?;
                }
            }
        }
    };
    proc_macro::TokenStream::from(expanded)
}

#[proc_macro_derive(ToPlist, attributes(fromplist))]
pub fn to_plist(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let serialize_cases = match add_serializecases(&input) {
        Ok(thing) => thing,
        Err(e) => return e.into_compile_error().into(),
    };
    let name = input.ident;

    let expanded = quote! {
        impl Into<Plist> for #name {
            fn into(self) -> Plist {
                let mut dict = crate::plist::Dictionary::new();
                #serialize_cases
                crate::plist::Plist::Dictionary(dict)
            }
        }
    };
    proc_macro::TokenStream::from(expanded)
}

fn fields_and_attrs(
    input: &DeriveInput,
) -> syn::Result<impl Iterator<Item = (&Field, FieldAttrs)>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "FromPlist only supports structs",
        ));
    };

    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            "FromPlist only supports named fields",
        ));
    };

    Ok(fields.named.iter().filter_map(|f| {
        attrs::FieldAttrs::from_attrs(&f.attrs)
            .ok()
            .filter(|a| !a.ignore)
            .map(|a| (f, a))
    }))
}

fn add_fieldcases(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = fields_and_attrs(input)?.flat_map(|(f, attrs)| {
            let name = f.ident.as_ref().unwrap();
            iter::once(
                attrs
                    .plist_field_name
                    .unwrap_or_else(|| snake_to_camel_case(&name.to_string())),
            )
            .chain(attrs.plist_addtl_names)
            .map(move |plist_name| {
                let name = name.clone();
                if attrs.other {
                    quote_spanned! {
                        f.span() => Some(unrecognized) => { rec.#name.insert(unrecognized.to_string(), tokenizer.parse()?); },
                    }
                } else {
                quote_spanned! {
                    f.span() => Some(#plist_name) => rec.#name = tokenizer.parse()?,
                }
            }
            })
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #( #fields )*
    })
}

fn add_serializecases(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = fields_and_attrs(input)?
        .flat_map(|(f, attrs)| {
            let name = f.ident.as_ref().unwrap();
            let plist_name = attrs
                .plist_field_name
                .unwrap_or_else(|| snake_to_camel_case(&name.to_string()));
            let name = name.clone();
            match &f.ty {
                Type::Path(typepath)
                    if typepath.qself.is_none() && path_is_option(&typepath.path) =>
                {
                    quote_spanned! {
                        f.span() => if let Some(inner) = self.#name {
                            dict.insert(#plist_name.into(), inner.into());
                        }
                    }
                }
                _ => {
                 if attrs.other {
                      quote_spanned! {
                          f.span() => dict.extend(self.#name.iter().map(|(k, v)| (k.into(), v.clone())));
                      }
                  } else {
                    quote_spanned! {
                        f.span() => 
                            #[allow(clippy::useless_conversion)]
                            dict.insert(#plist_name.into(), self.#name.into());
                    }
                  }
                }
            }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #( #fields )*
    })
}

fn snake_to_camel_case(id: &str) -> String {
    let mut result = String::new();
    let mut hump = false;
    for c in id.chars() {
        if c == '_' {
            hump = true;
        } else {
            if hump {
                result.push(c.to_ascii_uppercase());
            } else {
                result.push(c);
            }
            hump = false;
        }
    }
    result
}

fn path_is_option(path: &Path) -> bool {
    !path.segments.is_empty() && path.segments.iter().last().unwrap().ident == "Option"
}
