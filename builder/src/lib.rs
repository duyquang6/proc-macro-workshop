use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
    token::Comma,
    Data, DeriveInput, Expr, Fields, GenericArgument, Lit, Meta, PathArguments, Type,
};

struct BuilderFields {
    other_ident_fields: Vec<syn::Ident>,
    other_ty_fields: Vec<Type>,
    opt_ident_fields: Vec<syn::Ident>,
    opt_ty_fields: Vec<Type>,
    each_ident_fields: Vec<syn::Ident>,
    each_ident_fields_fullname: Vec<syn::Ident>,
    each_ty_fields: Vec<Type>,
}

impl BuilderFields {
    fn new() -> Self {
        Self {
            other_ident_fields: Vec::new(),
            other_ty_fields: Vec::new(),
            opt_ident_fields: Vec::new(),
            opt_ty_fields: Vec::new(),
            each_ident_fields: Vec::new(),
            each_ident_fields_fullname: Vec::new(),
            each_ty_fields: Vec::new(),
        }
    }

    fn collect_fields(&mut self, fields: &Fields) -> syn::Result<()> {
        if let Fields::Named(named_fields) = fields {
            for field in &named_fields.named {
                self.process_field(field)?;
            }
        }
        Ok(())
    }

    fn process_field(&mut self, field: &syn::Field) -> syn::Result<()> {
        let ident = match &field.ident {
            Some(ident) => ident,
            None => return Ok(()),
        };

        let (ty, is_opt) = self.get_field_type(&field.ty);
        let is_each = self.process_builder_attr(field, ident, ty)?;

        if is_opt {
            self.opt_ty_fields.push(ty.clone());
            self.opt_ident_fields.push(ident.clone());
        } else if !is_each {
            self.other_ty_fields.push(ty.clone());
            self.other_ident_fields.push(ident.clone());
        }

        Ok(())
    }

    fn get_field_type<'a>(&self, ty: &'a Type) -> (&'a Type, bool) {
        if let Some(inner_ty) = get_inner_type(ty, "Option") {
            (inner_ty, true)
        } else {
            (ty, false)
        }
    }

    fn process_builder_attr(
        &mut self,
        field: &syn::Field,
        ident: &syn::Ident,
        ty: &Type,
    ) -> syn::Result<bool> {
        let builder_attr = field
            .attrs
            .iter()
            .filter(|&attr| attr.path().is_ident("builder"))
            .last();

        let Some(builder_attr) = builder_attr else {
            return Ok(false);
        };

        let Meta::List(meta_list) = &builder_attr.meta else {
            return Ok(false);
        };

        let builder_args = match syn::parse2::<BuilderArgs>(meta_list.tokens.clone()) {
            Ok(args) => args,
            Err(e) => return Err(syn::Error::new(meta_list.span(), e)),
        };

        let Some(each) = builder_args.each else {
            return Ok(false);
        };

        self.each_ident_fields
            .push(quote::format_ident!("{}", each));
        self.each_ident_fields_fullname.push(ident.clone());
        if let Some(inner_ty) = get_inner_type(ty, "Vec") {
            self.each_ty_fields.push(inner_ty.clone());
        }
        Ok(true)
    }
}

#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;
    let builder_name = quote::format_ident!("{}Builder", struct_name);

    let mut builder_fields = BuilderFields::new();

    if let Data::Struct(struct_data) = &input.data {
        if let Err(e) = builder_fields.collect_fields(&struct_data.fields) {
            return e.into_compile_error().into();
        }
    }

    let BuilderFields {
        other_ident_fields,
        other_ty_fields,
        opt_ident_fields,
        opt_ty_fields,
        each_ident_fields,
        each_ident_fields_fullname,
        each_ty_fields,
    } = builder_fields;

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
                #(if self.#other_ident_fields.is_none() {
                    return Err(format!("field {} is empty", stringify!(#other_ident_fields)).into());
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
    let Type::Path(path) = &ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let Some(first_segment) = path.path.segments.first() else {
        return None;
    };
    if first_segment.ident != parent_ty {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &first_segment.arguments else {
        return None;
    };
    let Some(GenericArgument::Type(inner_ty)) = args.args.first() else {
        return None;
    };

    Some(inner_ty)
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
            let Expr::Assign(assign) = arg else {
                continue;
            };

            let span = assign.span();
            let Expr::Path(path) = *assign.left else {
                continue;
            };

            let Some(ident) = path.path.get_ident() else {
                continue;
            };

            if ident.to_string().as_str() != "each" {
                return Err(syn::Error::new(span, "expected `builder(each = \"...\")`"));
            }

            let Expr::Lit(lit) = *assign.right else {
                return Err(syn::Error::new(span, "expected `builder(each = \"...\")`"));
            };

            let Lit::Str(lit) = lit.lit else {
                return Err(syn::Error::new(span, "expected `builder(each = \"...\")`"));
            };

            each = Some(lit.value().trim_matches('"').to_string());
        }

        Ok(BuilderArgs { each })
    }
}
