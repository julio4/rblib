use super::*;

/// Iterator that yields `StepPath` values in traversal order for each
/// step in a pipeline and all its nested pipelines.
pub(crate) struct StepPathIter<'a, P: Platform> {
	stack: Vec<Frame<'a, P>>,
}

struct Frame<'a, P: Platform> {
	pipeline: &'a Pipeline<P>,
	path: StepPath,
	next_ix: usize,
	prologue_ix: usize,
	epilogue_ix: usize,
}

impl<'a, P: Platform> StepPathIter<'a, P> {
	pub(crate) fn new(pipeline: &'a Pipeline<P>) -> Self {
		Self {
			stack: vec![Frame {
				pipeline,
				next_ix: 0,
				path: StepPath::empty(),
				prologue_ix: 0,
				epilogue_ix: 0,
			}],
		}
	}
}

impl<P: Platform> Iterator for StepPathIter<'_, P> {
	type Item = StepPath;

	fn next(&mut self) -> Option<Self::Item> {
		loop {
			let frame = self.stack.last_mut()?;

			// Walk prologue steps
			if frame.prologue_ix < frame.pipeline.prologue.len() {
				let ix = frame.prologue_ix;
				frame.prologue_ix += 1;
				return Some(frame.path.clone().concat(StepPath::prologue_step(ix)));
			}

			// Walk steps; descend into nested pipelines.
			if frame.next_ix < frame.pipeline.steps.len() {
				let ix = frame.next_ix;
				frame.next_ix += 1;
				match &frame.pipeline.steps[ix] {
					StepOrPipeline::Step(_) => {
						return Some(frame.path.clone().concat(StepPath::step(ix)));
					}
					StepOrPipeline::Pipeline(_, nested) => {
						let next_path = frame.path.clone().concat(StepPath::step(ix));
						self.stack.push(Frame {
							pipeline: nested,
							path: next_path,
							next_ix: 0,
							prologue_ix: 0,
							epilogue_ix: 0,
						});
						continue;
					}
				}
			}

			// Walk epilogue steps
			if frame.epilogue_ix < frame.pipeline.epilogue.len() {
				let ix = frame.epilogue_ix;
				frame.epilogue_ix += 1;
				return Some(frame.path.clone().concat(StepPath::epilogue_step(ix)));
			}

			// Done with this frame; pop and continue with parent.
			self.stack.pop();
		}
	}
}

#[cfg(test)]
mod tests {
	use {super::*, crate::test_utils::fake_step};

	fake_step!(PrologueOne);
	fake_step!(PrologueTwo);

	fake_step!(EpilogueOne);
	fake_step!(EpilogueTwo);

	fake_step!(Step1);
	fake_step!(Step2);
	fake_step!(Step3);

	fake_step!(StepA);
	fake_step!(StepB);
	fake_step!(StepC);

	#[test]
	fn visits_all_steps_flat() {
		let pipeline = Pipeline::<Optimism>::default()
			.with_prologue(PrologueOne)
			.with_prologue(PrologueTwo)
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3)
			.with_epilogue(EpilogueOne)
			.with_epilogue(EpilogueTwo);

		let expected = vec!["p0", "p1", "0", "1", "2", "e0", "e1"];
		let actual = pipeline
			.iter_steps()
			.map(|step| step.to_string())
			.collect::<Vec<_>>();

		assert_eq!(actual, expected);
	}

	#[test]
	fn visits_all_steps_nested() {
		let pipeline = Pipeline::<Optimism>::default()
			.with_prologue(PrologueOne)
			.with_pipeline(Loop, |nested: Pipeline<Optimism>| {
				nested
					.with_prologue(PrologueTwo)
					.with_step(Step1)
					.with_pipeline(Loop, (StepA, StepB, StepC).with_epilogue(EpilogueTwo))
					.with_step(Step3)
			})
			.with_epilogue(EpilogueOne);

		let expected = vec![
			"p0", "0_p0", "0_0", "0_1_0", "0_1_1", "0_1_2", "0_1_e0", "0_2", "e0",
		];

		let actual = pipeline
			.iter_steps()
			.map(|step| step.to_string())
			.collect::<Vec<_>>();

		assert_eq!(actual, expected);
	}
}
