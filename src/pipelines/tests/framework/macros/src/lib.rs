use {
	proc_macro::TokenStream,
	quote::quote,
	syn::{
		Ident,
		ItemFn,
		Token,
		parse::{Parse, ParseStream},
		parse_macro_input,
		punctuated::Punctuated,
	},
};

struct PlatformList {
	platforms: Punctuated<Ident, Token![,]>,
}

impl Parse for PlatformList {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let platforms = input.parse_terminated(Ident::parse, Token![,])?;
		Ok(PlatformList { platforms })
	}
}

#[proc_macro_attribute]
pub fn rblib_test(args: TokenStream, input: TokenStream) -> TokenStream {
	let platform_list = parse_macro_input!(args as PlatformList);
	let original_fn = parse_macro_input!(input as ItemFn);

	let original_fn_name = &original_fn.sig.ident;
	let original_fn_block = &original_fn.block;
	let original_fn_attrs = &original_fn.attrs;

	// Extract function signature without the generic parameter
	let mut sig_without_generic = original_fn.sig.clone();
	sig_without_generic.generics = syn::Generics::default();

	// Generate test functions for each platform
	let test_functions = platform_list.platforms.iter().map(|platform| {
		let platform_lowercase = platform.to_string().to_lowercase();
		let test_fn_name = syn::Ident::new(
			&format!("{original_fn_name}_{platform_lowercase}"),
			original_fn_name.span(),
		);

		quote! {
				#[tokio::test]
				async fn #test_fn_name() {
						#original_fn_name::<#platform>().await
				}
		}
	});

	// Generate the original function (made private and generic)
	let expanded = quote! {
			#(#original_fn_attrs)*
			async fn #original_fn_name<P: TestablePlatform>() #original_fn_block

			#(#test_functions)*
	};

	TokenStream::from(expanded)
}
