use {
	proc_macro::TokenStream,
	quote::quote,
	syn::{parse_macro_input, LitInt},
};

/// Generates `IntoPipeline` implementations for tuples of steps up to the
/// specified count.
///
/// # Usage
/// ```rust
/// impl_into_pipeline_steps!(10);
/// ```
///
/// This will generate implementations for tuples of size 1 through 10.
#[proc_macro]
pub fn impl_into_pipeline_steps(input: TokenStream) -> TokenStream {
	let count = parse_macro_input!(input as LitInt);
	let count_value = count
		.base10_parse::<usize>()
		.expect("Expected a positive integer");

	let mut implementations = Vec::new();

	// Generate implementations for tuple sizes 1 through count
	for n in 1..=count_value {
		let implementation = generate_tuple_impl(n);
		implementations.push(implementation);
	}

	let expanded = quote! {
			#(#implementations)*
	};

	TokenStream::from(expanded)
}

/// Generates a single `IntoPipeline` implementation for a tuple of size `n`
fn generate_tuple_impl(n: usize) -> proc_macro2::TokenStream {
	// Generate mode type parameters: M0, M1, M2, ...
	let mode_params: Vec<_> = (0..n)
		.map(|i| {
			syn::Ident::new(&format!("M{}", i), proc_macro2::Span::call_site())
		})
		.collect();

	// Generate step type parameters: S0, S1, S2, ...
	let step_params: Vec<_> = (0..n)
		.map(|i| {
			syn::Ident::new(&format!("S{}", i), proc_macro2::Span::call_site())
		})
		.collect();

	// Generate destructuring pattern: (step0, step1, step2, ...)
	let step_vars: Vec<_> = (0..n)
		.map(|i| {
			syn::Ident::new(&format!("step{}", i), proc_macro2::Span::call_site())
		})
		.collect();

	// Generate chained with_step calls: .with_step(step0).with_step(step1)...
	let with_step_calls =
		step_vars
			.iter()
			.fold(quote! { Pipeline::default() }, |acc, step_var| {
				quote! { #acc.with_step(#step_var) }
			});

	quote! {
			impl<P: Platform, #(#mode_params: StepKind),*, #(#step_params: Step<Kind = #mode_params>),*> IntoPipeline<P, ()> for (#(#step_params),*) {
					fn into_pipeline(self) -> Pipeline<P> {
							let (#(#step_vars),*) = self;
							#with_step_calls
					}
			}
	}
}
