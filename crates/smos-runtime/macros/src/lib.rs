use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn smos_declare_main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemFn);

    let ident = &item.sig.ident;
    quote! {
        ::smos_runtime::smos_declare_main_internal!(#ident);

        #item
    }
    .into()
}
