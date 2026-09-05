//! Proc macros for BikesNest tests.
//!
//! `#[db_test]` marks an async test that receives a `&mut TestTx` (an open
//! PostgreSQL transaction provided by `bikesnest_test_support`). All
//! `#[db_test]`s share ONE multi-threaded tokio runtime and ONE connection
//! pool; the transaction is rolled back when the test ends.

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, parse_macro_input};

#[proc_macro_attribute]
pub fn db_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let name = &func.sig.ident;
    let body = &func.block;
    let vis = &func.vis;

    // Extract the single `tx` parameter (its path/type is not checked here;
    // the runner signature enforces `&mut TestTx`).
    let mut tx_ident: Option<proc_macro2::Ident> = None;
    for arg in &func.sig.inputs {
        if let FnArg::Typed(PatType { pat, .. }) = arg
            && let Pat::Ident(pat_ident) = &**pat
        {
            tx_ident = Some(pat_ident.ident.clone());
        }
    }

    let Some(tx) = tx_ident else {
        return syn::Error::new(
            func.sig.ident.span(),
            "#[db_test] requires exactly one argument `tx: &mut TestTx`",
        )
        .to_compile_error()
        .into();
    };

    let expanded = quote! {
        #[test]
        #vis fn #name() {
            bikesnest_test_support::run_db_test(async move |#tx| #body);
        }
    };
    expanded.into()
}
