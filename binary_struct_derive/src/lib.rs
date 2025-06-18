use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, Ident, Lit, Meta, MetaNameValue, NestedMeta,
    PathArguments, Type, TypePath,
};

#[proc_macro_derive(BinaryStruct, attributes(binary_struct))]
pub fn binary_struct_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = &input.ident;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => panic!("Only named fields are supported"),
        },
        _ => panic!("Only structs are supported"),
    };

    let mut field_reads = vec![];
    let mut struct_fields = vec![];
    let mut offset_expr = quote! { 0usize };

    let mut has_dynamic_field = false;

    for field in fields {
        let mut num_bytes_from: Option<Ident> = None;
        let mut encoding: Option<String> = None;
        let mut ignore = false;
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        // Check for #[binary_struct(num_bytes = N)] attribute on the field
        let mut num_bytes_opt = None;
        for attr in &field.attrs {
            if attr.path.is_ident("binary_struct") {
                if let Ok(Meta::List(meta_list)) = attr.parse_meta() {
                    for nested in meta_list.nested.iter() {
                        match nested {
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                path,
                                lit: Lit::Str(lit_str),
                                ..
                            })) if path.is_ident("num_bytes_from") => {
                                num_bytes_from = Some(Ident::new(&lit_str.value(), lit_str.span()));
                                has_dynamic_field = true;
                            }
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                path,
                                lit: Lit::Str(lit_str),
                                ..
                            })) if path.is_ident("encoding") => {
                                encoding = Some(lit_str.value());
                            }
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                path,
                                lit: Lit::Int(lit_int),
                                ..
                            })) if path.is_ident("num_bytes") => {
                                num_bytes_opt = Some(lit_int.base10_parse::<usize>().unwrap())
                            },
                            NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                                path,
                                lit: Lit::Bool(lit_bool),
                                ..
                            })) if path.is_ident("ignore") => {
                                ignore = lit_bool.value()
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if ignore {
            struct_fields.push(quote! {
                #field_name: ::core::default::Default::default()
            });
            continue;
        }

        // If the field type is a Skip<N> then the struct is skipped since it shouldn't have a size
        // The offset should also be advanced since these bytes are skipped
        if let Type::Path(TypePath { path, .. }) = field_type {
            if path.segments.len() == 1 && path.segments[0].ident == "Skip" {
                if let PathArguments::AngleBracketed(args) = &path.segments[0].arguments {
                    let n = &args.args[0];
                    field_reads.push(quote! {
                        let _ = &input[#offset_expr..#offset_expr + #n];
                    });
                    // Field still needs to exist since its defined in the original
                    // struct this macro is derived upon
                    struct_fields.push(quote! {
                        #field_name: ::binary_struct::Skip::<#n>::default()
                    });
                    offset_expr = quote! { #offset_expr + #n };
                    continue;
                }
            }
        }
        if let Some(num_bytes_from_field) = num_bytes_from.as_ref() {
            let tmp_var = format_ident!("__{}", field_name);
            let src_len = format_ident!("__{}", num_bytes_from_field);
            let offset = &offset_expr;

            let decode_expr = match encoding.as_deref() {
                Some("utf16") => quote! {
                    {
                        let byte_slice = &input[#offset..#offset + (#src_len as usize * 2usize)];
                        let u16_iter = byte_slice.chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]));
                        String::from_utf16(&u16_iter.collect::<Vec<_>>()).unwrap()
                    }
                },
                Some("utf8") | None => quote! {
                    {
                        let slice = &input[#offset..#offset + (#src_len as usize)];
                        ::core::str::from_utf8(slice).unwrap().to_string()
                    }
                },
                Some(enc) => panic!("Unsupported encoding: {}", enc),
            };
            field_reads.push(quote! {
                let #tmp_var = #decode_expr;
            });
            struct_fields.push(quote! {
                #field_name: #tmp_var
            });
        } else if let Some(num_bytes) = num_bytes_opt {
            let tmp_var = format_ident!("__{}", field_name);
            let offset = &offset_expr;
            let decode_expr = match encoding.as_deref() {
                Some("utf16") => quote! {
                    {
                        let byte_slice = &input[#offset..#offset + (#num_bytes as usize)];
                        let u16_iter = byte_slice.chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]));
                        String::from_utf16(&u16_iter.collect::<Vec<_>>()).unwrap_or_default()
                    }
                },
                Some("utf8") | None => quote! {
                    {
                        let slice = &input[#offset..#offset + (#num_bytes as usize)];
                        ::core::str::from_utf8(slice).unwrap_or_default().to_string()
                    }
                },
                Some(enc) => panic!("Unsupported encoding: {}", enc),
            };
            field_reads.push(quote! {
                let #tmp_var = #decode_expr;
            });
            struct_fields.push(quote! {
                #field_name: #tmp_var
            });
            offset_expr = quote! { #offset_expr + #num_bytes };
        } else {
            // Normal parse via BinaryParse
            let tmp_var = format_ident!("__{}", field_name);
            field_reads.push(quote! {
                let #tmp_var = {
                    let size = ::core::mem::size_of::<#field_type>();
                    let slice = &input[#offset_expr..#offset_expr + size];
                    <#field_type as ::binary_struct::BinaryParse>::parse(slice)?
                };
            });
            struct_fields.push(quote! {
                #field_name: #tmp_var
            });
            offset_expr = quote! {
                #offset_expr + <#field_type as BinarySize>::SIZE
            };
        }
    }

    let binary_size_impl = if !has_dynamic_field {
        Some(quote! {
            impl ::binary_struct::BinarySize for #struct_name {
                const SIZE: usize = #offset_expr;
            }
        })
    } else {
        None
    };

    let output = quote! {
        impl ::binary_struct::BinaryParse for #struct_name {
            fn parse(input: &[u8]) -> Result<Self, ::binary_struct::ParseError> {
                if input.len() < #offset_expr {
                    return Err(::binary_struct::ParseError::TooShort);
                }

                #(#field_reads)*

                Ok(Self {
                    #(#struct_fields),*
                })
            }
        }

        #binary_size_impl
    };

    TokenStream::from(output)
}
