use proc_macro::TokenStream;
use quote::format_ident;
use syn::{parse_macro_input, Fields, ItemStruct, PathArguments, Type, TypePath};
use quote::quote;

extern crate proc_macro;

#[proc_macro_attribute]
pub fn binary_struct(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemStruct);
    let struct_name = &input.ident;

    // Check if struct has debug derive
    let has_debug_derive = input.attrs.iter().any(|attr| {
        attr.path.is_ident("derive") && attr.tokens.to_string().contains("Debug")
    });

    // Add the debug derive if its not already present
    if !has_debug_derive {
        input.attrs.push(
            syn::parse_quote!(#[derive(Debug)])
        );
    }

    let fields = match &input.fields {
        Fields::Named(named) => &named.named,
        _ => panic!("Only named fields are supported"),
    };

    let mut field_reads = vec![];
    let mut struct_fields = vec![];
    let mut offset_expr = quote! { 0usize };

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

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

        // Determine real size of of field
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
            #offset_expr + ::core::mem::size_of::<#field_type>()
        };
    }

    let output = quote! {
        #input

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
    };

    TokenStream::from(output)
}