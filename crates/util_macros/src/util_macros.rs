#![cfg_attr(not(target_os = "windows"), allow(unused))]
#![allow(clippy::test_attr_in_doctest)]

use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{ItemFn, LitStr, parse_macro_input, parse_quote};

/// A macro used in tests for cross-platform path string literals in tests. On Windows it replaces
/// `/` with `\\` and adds `C:` to the beginning of absolute paths. On other platforms, the path is
/// returned unmodified.
///
/// # Example
/// ```rust
/// use util_macros::path;
///
/// let path = path!("/Users/user/file.txt");
/// #[cfg(target_os = "windows")]
/// assert_eq!(path, "C:\\Users\\user\\file.txt");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(path, "/Users/user/file.txt");
/// ```
#[proc_macro]
pub fn path(input: TokenStream) -> TokenStream {
    let path = parse_macro_input!(input as LitStr);
    let mut path = path.value();

    #[cfg(target_os = "windows")]
    {
        path = path.replace("/", "\\");
        if path.starts_with("\\") {
            path = format!("C:{}", path);
        }
    }

    TokenStream::from(quote! {
        #path
    })
}

/// This macro replaces the path prefix `file:///` with `file:///C:/` for Windows.
/// But if the target OS is not Windows, the URI is returned as is.
///
/// # Example
/// ```rust
/// use util_macros::uri;
///
/// let uri = uri!("file:///path/to/file");
/// #[cfg(target_os = "windows")]
/// assert_eq!(uri, "file:///C:/path/to/file");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(uri, "file:///path/to/file");
/// ```
#[proc_macro]
pub fn uri(input: TokenStream) -> TokenStream {
    let uri = parse_macro_input!(input as LitStr);
    let uri = uri.value();

    #[cfg(target_os = "windows")]
    let uri = uri.replace("file:///", "file:///C:/");

    TokenStream::from(quote! {
        #uri
    })
}

/// This macro replaces the line endings `\n` with `\r\n` for Windows.
/// But if the target OS is not Windows, the line endings are returned as is.
///
/// # Example
/// ```rust
/// use util_macros::line_endings;
///
/// let text = line_endings!("Hello\nWorld");
/// #[cfg(target_os = "windows")]
/// assert_eq!(text, "Hello\r\nWorld");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(text, "Hello\nWorld");
/// ```
#[proc_macro]
pub fn line_endings(input: TokenStream) -> TokenStream {
    let text = parse_macro_input!(input as LitStr);
    let text = text.value();

    #[cfg(target_os = "windows")]
    let text = text.replace("\n", "\r\n");

    TokenStream::from(quote! {
        #text
    })
}

/// Marks a test as perf-sensitive. This also automatically applies `#[test]`.
///
/// Accepts optional importance level arguments (`critical`, `important`, `average`,
/// `iffy`, `fluff`) and `weight = N` / `iterations = N` parameters, but these are
/// currently no-ops.
///
/// # Examples
/// ```rust
/// use util_macros::perf;
///
/// #[perf]
/// fn generic_test() {
///     // Test goes here.
/// }
///
/// #[perf(fluff, weight = 30)]
/// fn cold_path_test() {
///     // Test goes here.
/// }
/// ```
#[proc_macro_attribute]
pub fn perf(our_attr: TokenStream, input: TokenStream) -> TokenStream {
    // Parse and discard attribute arguments (iterations, weight, importance levels)
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("iterations") || meta.path.is_ident("weight") {
            let _: syn::Expr = meta.value()?.parse()?;
        } else if !(meta.path.is_ident("critical")
            || meta.path.is_ident("important")
            || meta.path.is_ident("average")
            || meta.path.is_ident("iffy")
            || meta.path.is_ident("fluff"))
        {
            return Err(syn::Error::new_spanned(meta.path, "unexpected identifier"));
        }
        Ok(())
    });
    parse_macro_input!(our_attr with parser);

    let ItemFn {
        attrs: mut attrs_main,
        vis,
        sig: sig_main,
        block,
    } = parse_macro_input!(input as ItemFn);
    if !attrs_main
        .iter()
        .any(|a| Some(&parse_quote!(test)) == a.path().segments.last())
    {
        attrs_main.push(parse_quote!(#[test]));
    }
    attrs_main.push(parse_quote!(#[allow(non_snake_case)]));

    let f = ItemFn {
        attrs: attrs_main,
        vis,
        sig: sig_main,
        block,
    };

    TokenStream::from(f.into_token_stream())
}
