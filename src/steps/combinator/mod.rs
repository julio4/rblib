//! This module defines combinator steps that can be used to combine multiple
//! steps into a single step.

use {crate::prelude::*, std::sync::Arc};

type Steps<P: Platform> = Vec<Arc<StepInstance<P>>>;

pub trait CombinatorStep<P: Platform>: Step<P> {
	fn of(steps: impl Into<Steps<P>>) -> Self;
}

macro_rules! combinator {
	// name is the combinator struct name,
	// append the method name of adding a step (optional)
	($name:ident, $append:ident) => {
		pub struct $name<P: Platform>(pub Steps<P>);

		impl<P: Platform> CombinatorStep<P> for $name<P> {
			fn of(steps: impl Into<Steps<P>>) -> Self {
				Self(steps.into())
			}
		}

		impl<P: Platform> $name<P> {
			#[must_use]
			#[allow(unused)]
			fn $append(mut self, other: impl Step<P>) -> Self {
				self.0.push(Arc::new(StepInstance::new(other)));
				self
			}

			fn steps(&self) -> &[Arc<StepInstance<P>>] {
				&self.0
			}
		}

		// TODO: if macro_metavar_expr feature is stabilized, we can generate macro
		// automatically: #[macro_export]
		// macro_rules! $macro_name {
		//     ($$first:expr $(, $$rest:expr)* $(,)?) => {{
		//         let mut c =
		//             $name::of(vec![Arc::new(StepInstance::new($$first))]);
		//         $(
		//             c = c.$append($$rest);
		//         )*
		//         c
		//     }};
		// }
	};

	// default method name for adding a step: append
	($name:ident) => {
		combinator!($name, append);
	};
}

mod atomic;
mod chain;

pub use {atomic::Atomic, chain::Chain};
