use {super::*, std::sync::Arc};

pub struct AtomicMode;

impl<P: Platform> CompositeStepMode<P> for AtomicMode {
	async fn steps(
		&self,
		steps: &[Arc<StepInstance<P>>],
		payload: Checkpoint<P>,
		ctx: StepContext<P>,
	) -> ControlFlow<P> {
		let initial = payload.clone();
		let mut current = payload;

		for step in steps {
			if ctx.deadline_reached() {
				return ControlFlow::Break(initial);
			}

			match step.step(current, ctx.clone()).await {
				ControlFlow::Ok(next) => current = next,
				ControlFlow::Break(_) => return ControlFlow::Break(initial),
				ControlFlow::Fail(error) => return ControlFlow::Fail(error),
			}
		}

		if ctx.deadline_reached() {
			ControlFlow::Break(initial)
		} else {
			ControlFlow::Ok(current)
		}
	}
}

#[macro_export]
macro_rules! atomic {
    ($($step:expr),+ $(,)?) => {{
        let mut composite =
            $crate::prelude::composite::CompositeStep::new(
                $crate::prelude::composite::atomic::AtomicMode,
            );
        $(
            composite.append_step($step);
        )+
        composite
    }};
}

#[cfg(test)]
mod tests {
	use {super::*, crate::test_utils::*};

	// TODO: improve tests here

	#[rblib_test(Ethereum)]
	async fn atomic_mode_break_reverts<P: TestablePlatform>() -> eyre::Result<()>
	{
		let mut composite = CompositeStep::<P, _>::new(AtomicMode);
		composite.append_step(AlwaysOkStep);
		composite.append_step(AlwaysBreakStep); // break here
		composite.append_step(AlwaysOkStep);

		let result = OneStep::<P>::new(composite).run().await?;
		assert!(matches!(result, ControlFlow::Break(_)));

		Ok(())
	}

	#[rblib_test(Ethereum)]
	async fn atomic_mode_fail_propagates<P: TestablePlatform>() -> eyre::Result<()>
	{
		let mut composite = CompositeStep::<P, _>::new(AtomicMode);
		composite.append_step(AlwaysOkStep);
		composite.append_step(AlwaysFailStep);
		composite.append_step(AlwaysOkStep);

		let result = OneStep::<P>::new(composite).run().await?;
		assert!(matches!(result, ControlFlow::Fail(_)));

		Ok(())
	}

	#[rblib_test(Ethereum)]
	async fn atomic_macro_basic<P: TestablePlatform>() -> eyre::Result<()> {
		let composite = atomic!(AlwaysOkStep, AlwaysOkStep);

		let result = OneStep::<P>::new(composite).run().await?;
		assert!(matches!(result, ControlFlow::Ok(_)));

		Ok(())
	}

	#[rblib_test(Ethereum)]
	async fn atomic_in_pipeline<P: TestablePlatform>() -> eyre::Result<()> {
		let pipeline =
			Pipeline::<P>::default().with_step(atomic!(AlwaysOkStep, AlwaysOkStep));

		P::create_test_node(pipeline).await?.next_block().await?;

		Ok(())
	}
}
