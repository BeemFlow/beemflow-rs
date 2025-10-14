//! Procedural macros for BeemFlow core operations
//!
//! Provides `#[operation]` and `#[operation_group]` macros that automatically:
//! - Generate operation metadata
//! - Register operations for CLI/HTTP/MCP interfaces
//! - Eliminate magic strings and duplication

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Ident, ItemMod, ItemStruct, LitStr, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

/// Parse attribute arguments for operation_group
struct OperationGroupArgs {
    group_name: Ident,
}

impl Parse for OperationGroupArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let group_name: Ident = input.parse()?;
        Ok(OperationGroupArgs { group_name })
    }
}

/// Attribute macro for operation groups
///
/// Usage: #[operation_group(flows)]
#[proc_macro_attribute]
pub fn operation_group(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as OperationGroupArgs);
    let input = parse_macro_input!(item as ItemMod);

    let group_name = args.group_name.to_string();
    let mod_name = &input.ident;
    let vis = &input.vis;

    // Extract module content
    let (_brace, items) = match input.content {
        Some((brace, items)) => (brace, items),
        None => {
            return syn::Error::new_spanned(input, "Module must have content")
                .to_compile_error()
                .into();
        }
    };

    // Find all struct items that are actual operations (have a 'deps' field)
    let operation_structs: Vec<&Ident> = items
        .iter()
        .filter_map(|item| {
            if let syn::Item::Struct(s) = item {
                // Check if this struct has a 'deps' field
                let has_deps_field = if let syn::Fields::Named(fields) = &s.fields {
                    fields
                        .named
                        .iter()
                        .any(|f| f.ident.as_ref().map(|i| i == "deps").unwrap_or(false))
                } else {
                    false
                };

                if has_deps_field { Some(&s.ident) } else { None }
            } else {
                None
            }
        })
        .collect();

    // Generate registration calls for all structs
    let registration_calls = operation_structs.iter().map(|struct_name| {
        quote! {
            registry.register(
                #struct_name::new(deps.clone()),
                #struct_name::OPERATION_NAME,
            );
        }
    });

    // Pass through the module with added metadata and auto-registration
    let expanded = quote! {
        #vis mod #mod_name {
            pub const GROUP_NAME: &str = #group_name;

            #(#items)*

            /// Auto-generated function to register all operations in this group
            pub fn register_all(
                registry: &mut super::super::OperationRegistry,
                deps: std::sync::Arc<super::Dependencies>,
            ) {
                #(#registration_calls)*
            }
        }
    };

    TokenStream::from(expanded)
}

/// Parse attribute arguments for operation
struct OperationArgs {
    name: Option<String>,
    http: Option<String>,
    cli: Option<String>,
    description: Option<String>,
    group: Option<String>,
    input: Option<Ident>,
}

impl Parse for OperationArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut http = None;
        let mut cli = None;
        let mut description = None;
        let mut group = None;
        let mut input_type = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match ident.to_string().as_str() {
                "input" => {
                    // Parse identifier for input type
                    let type_ident: Ident = input.parse()?;
                    input_type = Some(type_ident);
                }
                _ => {
                    let value: LitStr = input.parse()?;
                    match ident.to_string().as_str() {
                        "name" => name = Some(value.value()),
                        "http" => http = Some(value.value()),
                        "cli" => cli = Some(value.value()),
                        "description" => description = Some(value.value()),
                        "group" => group = Some(value.value()),
                        _ => return Err(syn::Error::new_spanned(ident, "Unknown attribute")),
                    }
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(OperationArgs {
            name,
            http,
            cli,
            description,
            group,
            input: input_type,
        })
    }
}

/// Convert PascalCase to snake_case
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();

    for ch in s.chars() {
        if ch.is_uppercase() {
            if !result.is_empty() {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }

    result
}

/// Parse HTTP method and path from string like "GET /flows/{name}"
fn parse_http_route(http: &str) -> (String, String) {
    let parts: Vec<&str> = http.splitn(2, ' ').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("GET".to_string(), http.to_string())
    }
}

/// Attribute macro for individual operations
///
/// Usage: #[operation(name = "get_flow", http = "GET /flows/{name}", cli = "get <NAME>")]
#[proc_macro_attribute]
pub fn operation(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as OperationArgs);
    let input = parse_macro_input!(item as ItemStruct);

    let struct_name = &input.ident;
    let vis = &input.vis;
    let fields = &input.fields;
    let attrs = &input.attrs;

    // Determine operation name
    let operation_name = args
        .name
        .unwrap_or_else(|| to_snake_case(&struct_name.to_string()));

    // Extract doc comments for description (fallback)
    let doc_comment = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc")
                && let syn::Meta::NameValue(meta) = &attr.meta
                && let syn::Expr::Lit(expr_lit) = &meta.value
                && let syn::Lit::Str(lit_str) = &expr_lit.lit
            {
                return Some(lit_str.value().trim().to_string());
            }
            None
        })
        .collect::<Vec<_>>()
        .join(" ");

    let description = args.description.unwrap_or_else(|| {
        if doc_comment.is_empty() {
            format!("{} operation", struct_name)
        } else {
            doc_comment
        }
    });

    // Parse HTTP metadata
    let (http_method, http_path) = if let Some(http) = args.http {
        let (method, path) = parse_http_route(&http);
        (quote! { Some(#method) }, quote! { Some(#path) })
    } else {
        (quote! { None }, quote! { None })
    };

    // CLI metadata
    let cli_pattern = if let Some(cli) = args.cli {
        quote! { Some(#cli) }
    } else {
        quote! { None }
    };

    // Group metadata - use explicit group or fall back to GROUP_NAME
    let group_value = if let Some(group) = args.group {
        quote! { #group }
    } else {
        quote! { GROUP_NAME }
    };

    // Generate schema from Input type if provided, with OnceLock caching
    let schema_generation = if let Some(input_type) = args.input {
        quote! {
            {
                use std::sync::OnceLock;
                use schemars::schema_for;

                static SCHEMA: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();

                SCHEMA.get_or_init(|| {
                    let schema = schema_for!(#input_type);
                    match serde_json::to_value(&schema) {
                        Ok(serde_json::Value::Object(map)) => map,
                        _ => {
                            // Fallback to empty schema if conversion fails
                            let mut schema = serde_json::Map::new();
                            schema.insert("type".to_string(), serde_json::Value::String("object".to_string()));
                            schema.insert("properties".to_string(), serde_json::Value::Object(serde_json::Map::new()));
                            schema.insert("additionalProperties".to_string(), serde_json::Value::Bool(true));
                            schema
                        }
                    }
                }).clone()
            }
        }
    } else {
        // Default fallback schema if no input type specified
        quote! {
            {
                use std::sync::OnceLock;

                static SCHEMA: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();

                SCHEMA.get_or_init(|| {
                    let mut schema = serde_json::Map::new();
                    schema.insert("type".to_string(), serde_json::Value::String("object".to_string()));
                    schema.insert("properties".to_string(), serde_json::Value::Object(serde_json::Map::new()));
                    schema.insert("additionalProperties".to_string(), serde_json::Value::Bool(true));
                    schema
                }).clone()
            }
        }
    };

    // Generate operation metadata
    let expanded = quote! {
        #[derive(Clone)]
        #(#attrs)*
        #vis struct #struct_name #fields

        impl #struct_name {
            pub const OPERATION_NAME: &'static str = #operation_name;
            pub const DESCRIPTION: &'static str = #description;
            pub const GROUP: &'static str = #group_value;
            pub const HTTP_METHOD: Option<&'static str> = #http_method;
            pub const HTTP_PATH: Option<&'static str> = #http_path;
            pub const CLI_PATTERN: Option<&'static str> = #cli_pattern;

            pub fn new(deps: std::sync::Arc<super::Dependencies>) -> Self {
                Self { deps }
            }
        }

        impl super::super::HasMetadata for #struct_name {
            fn metadata() -> super::super::OperationMetadata {
                super::super::OperationMetadata {
                    name: Self::OPERATION_NAME,
                    description: Self::DESCRIPTION,
                    group: Self::GROUP,
                    http_method: Self::HTTP_METHOD,
                    http_path: Self::HTTP_PATH,
                    cli_pattern: Self::CLI_PATTERN,
                    schema: #schema_generation,
                }
            }
        }
    };

    TokenStream::from(expanded)
}
