use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
    token::Comma,
    Data, DeriveInput, Expr, GenericArgument, Lit, Meta, PathArguments, Type,
};

#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = &input.ident;
    let builder_name = quote::format_ident!("{}Builder", struct_name);
    let mut other_ident_fields = Vec::new();
    let mut other_ty_fields = Vec::new();
    let mut opt_ident_fields = Vec::new();
    let mut opt_ty_fields = Vec::new();
    let mut each_ident_fields = Vec::new();
    let mut each_ident_fields_fullname = Vec::new();
    let mut each_ty_fields = Vec::new();

    if let Data::Struct(struct_data) = &input.data {
        for field in &struct_data.fields {
            if let Some(ident) = &field.ident {
                let mut is_opt = false;
                let mut ty = &field.ty;

                if let Some(inner_ty) = get_inner_type(ty, "Option") {
                    ty = inner_ty;
                    is_opt = true;
                }

                // check repetition field
                let builder_attr_opt = field
                    .attrs
                    .iter()
                    .filter(|&attr| attr.path().is_ident("builder"))
                    .last();

                let mut is_each = false;
                if let Some(builder_attr) = builder_attr_opt {
                    if let Meta::List(meta_list) = &builder_attr.meta {
                        let builder_args: syn::Result<BuilderArgs> =
                            syn::parse2(meta_list.tokens.clone());

                        if let Err(e) = builder_args {
                            return syn::Error::new(
                                meta_list.span(),
                                "expected `builder(each = \"...\")`",
                            )
                            .into_compile_error()
                            .into();
                        }

                        if let Some(each) = builder_args.unwrap().each {
                            is_each = true;
                            each_ident_fields.push(quote::format_ident!("{}", each));
                            each_ident_fields_fullname.push(ident);
                            if let Some(inner_ty) = get_inner_type(ty, "Vec") {
                                each_ty_fields.push(inner_ty);
                            }
                        }
                    }
                }

                if is_opt {
                    opt_ty_fields.push(ty);
                    opt_ident_fields.push(ident);
                } else if is_each {
                    // TODO: handle each field
                } else {
                    other_ty_fields.push(ty);
                    other_ident_fields.push(ident);
                }
            }
        }
    }

    let expanded = quote! {
        impl #struct_name {
            pub fn builder() -> #builder_name {
                #builder_name {
                    #(#other_ident_fields: std::option::Option::None,)*
                    #(#opt_ident_fields: std::option::Option::None,)*
                    #(#each_ident_fields: std::vec::Vec::new(),)*
                }
            }
        }

        pub struct #builder_name {
            #(#other_ident_fields: std::option::Option<#other_ty_fields>,)*
            #(#opt_ident_fields: std::option::Option<#opt_ty_fields>,)*
            #(#each_ident_fields: std::vec::Vec<#each_ty_fields>,)*
        }

        impl #builder_name {
            #(fn #other_ident_fields(&mut self, #other_ident_fields: #other_ty_fields) -> &mut Self {
                self.#other_ident_fields = std::option::Option::Some(#other_ident_fields);
                self
            })*

            #(fn #opt_ident_fields(&mut self, #opt_ident_fields: #opt_ty_fields) -> &mut Self {
                self.#opt_ident_fields = std::option::Option::Some(#opt_ident_fields);
                self
            })*

            #(fn #each_ident_fields(&mut self, #each_ident_fields: #each_ty_fields) -> &mut Self {
                self.#each_ident_fields.push(#each_ident_fields);
                self
            })*

            pub fn build(&mut self) -> std::result::Result<#struct_name, std::boxed::Box<dyn std::error::Error>> {
                // check all field exist
                #(if self.#other_ident_fields.is_none() {
                    Err(format!("field {} is empty", stringify!(#other_ident_fields)))?;
                })*


                Ok(#struct_name {
                    #(#other_ident_fields: self.#other_ident_fields.take().unwrap(),)*
                    #(#opt_ident_fields: self.#opt_ident_fields.take(),)*
                    #(#each_ident_fields_fullname: self.#each_ident_fields.clone(),)*
                })
            }
        }
    };

    TokenStream::from(expanded)
}

fn get_inner_type<'a>(ty: &'a Type, parent_ty: &'static str) -> Option<&'a Type> {
    if let Type::Path(path) = &ty {
        if path.qself.is_some() {
            return None;
        }

        if let Some(first_segment) = path.path.segments.first() {
            if first_segment.ident == parent_ty {
                if let PathArguments::AngleBracketed(args) = &first_segment.arguments {
                    if let GenericArgument::Type(inner_ty) = args.args.first().unwrap() {
                        return Some(inner_ty);
                    }
                }
            }
        }
    }
    None
}

// Define a struct to parse the arguments
struct BuilderArgs {
    each: Option<String>,
}

impl Parse for BuilderArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let args = Punctuated::<Expr, Comma>::parse_terminated(input)?;
        let mut each = None;

        for arg in args {
            if let Expr::Assign(assign) = arg {
                let span = assign.span();
                if let Expr::Path(path) = *assign.left {
                    if let Some(ident) = path.path.get_ident() {
                        match ident.to_string().as_str() {
                            "each" => {
                                if let Expr::Lit(lit) = *assign.right {
                                    if let Lit::Str(lit) = lit.lit {
                                        // trim quote
                                        each = Some(lit.value().trim_matches('"').to_string());
                                    }
                                }
                            }
                            _ => {
                                return Err(syn::Error::new(span, "unrecognized attribute"));
                            }
                        }
                    }
                }
            }
        }

        Ok(BuilderArgs { each })
    }
}
