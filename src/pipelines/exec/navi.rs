//! Pipeline navigation
//!
//! Types in this module are responsible for navigating through the pipeline
//! steps, They identify next steps, manage loops, and handle nested pipelines,
//! etc.

use {
	crate::{pipelines::step::WrappedStep, *},
	derive_more::{From, Into},
	smallvec::{smallvec, SmallVec},
	std::sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
	Path(StepPath),
	Failure,
	Completed,
}

/// Represents a path to a step or a nested pipeline in a pipeline.
///
/// This type is used to store the current position in the pipeline execution
/// and knows how to navigate through the pipeline structure depending on the
/// current step output and the pipeline structure.
///
/// A Step path cannot be empty, it must always contain at least one element,
/// which is the case for pipelines with only steps and no nested pipelines.
#[derive(PartialEq, Eq, Clone, Debug, From, Into)]
pub(crate) struct StepPath(SmallVec<[usize; 8]>);

const PROLOGUE_INDEX: usize = usize::MIN;
const EPILOGUE_INDEX: usize = usize::MAX;
const STEP0_INDEX: usize = PROLOGUE_INDEX + 1;

impl StepPath {
	/// Constructs a new step path from a list of indices.
	pub fn new(path: impl Into<SmallVec<[usize; 8]>>) -> Self {
		let path: SmallVec<[usize; 8]> = path.into();
		assert!(!path.is_empty(), "StepPath cannot be empty");
		Self(path)
	}

	/// Returns the number of nesting levels in the path.
	///
	/// When this path points to an item, this value is the number of pipelines
	/// that contain the item starting from the top-level pipeline.
	pub fn depth(&self) -> usize {
		self.0.len()
	}

	/// Returns `true` if the path points to a step in a top-level pipeline.
	/// This means that this path is inside a pipeline that has no parents.
	///
	/// In other words, it checks if the path is a single element path.
	pub fn is_toplevel(&self) -> bool {
		self.depth() == 1
	}

	/// Returns `true` the the path is pointing to a prologue of a pipeline.
	pub fn is_prologue(&self) -> bool {
		self.0.last() == Some(&PROLOGUE_INDEX)
	}

	/// Returns `true` if the path is pointing to an epilogue of a pipeline.
	pub fn is_epilogue(&self) -> bool {
		self.0.last() == Some(&EPILOGUE_INDEX)
	}

	/// Returns the index of the root element in the path.
	///
	/// For example, if the path is `[3, 1, 2]`, this function will return `0`,
	/// which represents the nested pipeline at index `3`. A path of `[1]`
	/// represents the step or nested pipeline at index `1` in the pipeline.
	pub fn root(&self) -> Option<usize> {
		self.0.first().copied()
	}

	/// Returns the index of the step or nested pipeline pointed to by the
	/// path relative to its immediate parent.
	///
	/// This essentially returns the last element of the path.
	pub fn leaf(&self) -> usize {
		self.0.last().copied().expect("StepPath cannot be empty")
	}

	/// Returns a step path without the current root index.
	///
	/// This is useful when doing recursive navigation down through the pipeline.
	/// Returns None if the path is an item in a top-level pipeline.
	pub fn remove_root(self) -> Option<StepPath> {
		if self.is_toplevel() {
			// If there's only one element, popping it would result in an empty path
			// which is not allowed.
			None
		} else {
			Some(StepPath(self.0[1..].into()))
		}
	}

	/// Returns a step path without the current leaf index.
	///
	/// This is useful when you want to navigate up to the pipeline or step that
	/// contains the current step or pipeline.
	///
	/// Returns None if the path points to a top-level item.
	pub fn remove_leaf(self) -> Option<StepPath> {
		if self.is_toplevel() {
			None
		} else {
			Some(StepPath(self.0[..self.0.len() - 1].into()))
		}
	}

	/// Appends a new path to the current path.
	pub fn concat(self, other: Self) -> Self {
		let mut new_path = self.0;
		new_path.extend(other.0);
		Self(new_path)
	}

	/// Returns a step path that points to the prologue step.
	fn prologue() -> Self {
		Self(smallvec![PROLOGUE_INDEX])
	}

	fn epilogue() -> Self {
		Self(smallvec![EPILOGUE_INDEX])
	}

	/// Returns a new step path that points to the first non-prologue and
	/// non-epilogue step.
	fn step0() -> Self {
		Self::step(0)
	}

	fn step(step_index: usize) -> Self {
		Self(smallvec![step_index + STEP0_INDEX])
	}

	/// Returns a path that points to the next item in the current scope.
	///
	/// This method does not check if the next item is valid or exists in the
	/// pipeline. It simply increments the last index in the path.
	fn increment_leaf(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last += 1;
		}
		Self(new_path)
	}

	fn replace_leaf(self, new_leaf: usize) -> Self {
		let mut new_path = self.0;
		*new_path.last_mut().expect("StepPath cannot be empty") = new_leaf;
		Self(new_path)
	}

	fn append_prologue(self) -> Self {
		self.concat(StepPath::prologue())
	}

	fn append_epilogue(self) -> Self {
		self.concat(StepPath::epilogue())
	}

	fn append_step(self, step_index: usize) -> Self {
		self.concat(StepPath::step(step_index))
	}
}

/// This type is used to navigate through a pipeline.
/// It keeps track of the current step and the hierarchy of enclosing pipelines.
/// The path in this type always points at a step, so it traverses only leaves
/// of the pipeline tree.
#[derive(Clone)]
pub(crate) struct StepNavigator<'a, P: Platform>(
	StepPath,
	Vec<&'a Pipeline<P>>,
);

impl<'a, P: Platform> StepNavigator<'a, P> {
	/// Given a pipeline, returns a navigator that points at the first executable
	/// item in the pipeline.
	///
	/// In pipelines with a prologue, this will point to the prologue step.
	/// In pipelines without a prologue, this will point to the first step.
	/// In pipelines with no steps, but with an epilogue, this will point to the
	/// epilogue step.
	///
	/// If the first item in the pipeline is a nested pipeline, this will dig
	/// deeper into the nested pipeline to find the first executable item.
	///
	/// In empty pipelines, this will return None.
	pub fn entrypoint(pipeline: &'a Pipeline<P>) -> Option<Self> {
		if pipeline.is_empty() {
			return None;
		}

		// pipeline has a prologue, return it.
		if pipeline.prologue().is_some() {
			return Some(Self(StepPath::prologue(), vec![pipeline]));
		}

		// pipeline has no prologue
		if pipeline.steps().is_empty() {
			// If there are no steps, but there is an epilogue, return it.
			if pipeline.epilogue().is_some() {
				return Some(Self(StepPath::epilogue(), vec![pipeline]));
			} else {
				// this is an empty pipeline, there is nothing executable.
				return None;
			}
		}

		// pipeline has steps, dig into the entrypoint of the first item
		Self(StepPath::step0(), vec![pipeline]).enter()
	}

	/// Returns a reference to the instance of the step that this path is
	/// currently pointing to.
	pub fn step(&self) -> &Arc<WrappedStep<P>> {
		let step_index = self.0.leaf();
		let enclosing_pipeline = self.1.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		if step_index == PROLOGUE_INDEX {
			enclosing_pipeline
				.prologue()
				.expect("Step path points to a non-existing prologue")
		} else if step_index == EPILOGUE_INDEX {
			enclosing_pipeline
				.epilogue()
				.expect("Step path points to a non-existing epilogue")
		} else {
			let StepOrPipeline::Step(step) = enclosing_pipeline
				.steps()
				.get(step_index - STEP0_INDEX)
				.expect("Step path points to a non-existing step")
			else {
				unreachable!(
					"StepNavigator should not point to a pipeline, only to steps"
				)
			};

			step
		}
	}

	/// Advance to the next executable step in the pipeline when the current
	/// step's execution returned `ControlFlow::Ok`.
	///
	/// Returns `None` if there are no more steps to execute in the pipeline.
	pub fn next_ok(self) -> Option<Self> {
		if self.is_epilogue() {
			// the loop is over.
			return self.next_in_parent();
		}

		if self.is_prologue() {
			// start looping (if possible)
			return self.after_prologue();
		}

		let enclosing_pipeline = self.1.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		// we are in a regular step.
		assert!(
			!enclosing_pipeline.steps().is_empty(),
			"invalid navigator state"
		);

		let position = self
			.0
			.leaf()
			.checked_sub(STEP0_INDEX)
			.expect("invalid step index in step path");

		let is_last = position + 1 >= enclosing_pipeline.steps().len();

		match (self.behavior(), is_last) {
			(Behavior::Loop, true) => {
				// we are the last step in a loop pipeline, go to first step.
				Self(self.0.replace_leaf(STEP0_INDEX), self.1.clone()).enter()
			}
			(Behavior::Once, true) => {
				// we are last step in a non-loop pipeline, this is the end of a
				// single iteration loop.
				self.after_loop()
			}
			(_, false) => {
				// if we are not the last step of the pipeline, just go to the
				// next step.
				Self(self.0.increment_leaf(), self.1.clone()).enter()
			}
		}
	}

	/// Advance to the next executable step in the pipeline when the current
	/// step's execution returned `ControlFlow::Break`.
	///
	/// Returns `None` if there are no more steps to execute in the pipeline.
	pub fn next_break(self) -> Option<Self> {
		if self.is_epilogue() {
			// the loop is over.
			return self.next_in_parent();
		}

		// don't run any further steps in the current scope
		self.after_loop()
	}

	/// Returns the loop behaviour of the pipeline containing the current step.
	fn behavior(&self) -> Behavior {
		// top-level pipelines are always `Once`.
		if self.0.is_toplevel() {
			return Behavior::Once;
		}

		// to identify the behavior of the pipeline that contains the current step
		// we need to look at the grandparent pipeline, which contains the immediate
		// parent pipeline that contains the current step, and check the behavior
		// it is configured with.

		let parent_path = self.0.clone().remove_leaf().expect("non-top-level");
		let grandparent_pipeline =
			self.1.iter().rev().nth(1).expect("non-top-level");

		let parent_index = parent_path
			.leaf()
			.checked_sub(STEP0_INDEX)
			.expect("step ancestors may not be prologues or epilogues");

		let StepOrPipeline::Pipeline(behavior, _) = grandparent_pipeline
			.steps()
			.get(parent_index)
			.expect("parent pipeline should contain the current step")
		else {
			unreachable!("all ancestors of a step must be pipelines");
		};

		*behavior
	}

	/// Creates a new navigator that points to the first ancestor of the current
	/// item pointed to by the path.
	fn ancestor(self) -> Option<Self> {
		let StepNavigator(path, ancestors) = self;
		let path = path.remove_leaf()?;
		let ancestors = ancestors[0..ancestors.len() - 1].to_vec();
		Some(StepNavigator(path, ancestors))
	}

	/// Creates a new navigator that points to the entrypoint of the element
	/// pointed to by the current path.
	///
	/// If the current path points to a step, this will return itself.
	/// Returns None if the current path points to an empty nested pipeline.
	fn enter(self) -> Option<Self> {
		let StepNavigator(path, ancestors) = self;
		let enclosing_pipeline = ancestors.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		if path.is_prologue() || path.is_epilogue() {
			assert!(
				enclosing_pipeline.prologue().is_some()
					|| enclosing_pipeline.epilogue().is_some(),
				"path is prologue or epilogue, but enclosing pipeline has none",
			);
			// if we are in a prologue or epilogue, we can just return ourselves.
			return Some(Self(path, ancestors));
		}

		let step_index = path
			.leaf()
			.checked_sub(STEP0_INDEX)
			.expect("path is not prologue");

		match enclosing_pipeline.steps().get(step_index)? {
			StepOrPipeline::Step(_) => {
				// if we are pointing at a step, we can just return ourselves.
				Some(Self(path, ancestors))
			}
			StepOrPipeline::Pipeline(_, nested) => {
				// if we are pointing at a pipeline, we need to dig into its entrypoint.
				Some(StepNavigator(path, ancestors).join(Self::entrypoint(nested)?))
			}
		}
	}

	/// Finds the next step to run when a loop is finished.
	///
	/// The next step could be either the epilogue of the current pipeline,
	/// or the next step in the parent pipeline.
	fn after_loop(self) -> Option<Self> {
		let enclosing_pipeline = self.1.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		if enclosing_pipeline.epilogue().is_some() {
			// we've reached the epilogue of this pipeline, regardless of the
			// looping behavior, we should go to the next step in the parent pipeline.
			Some(Self(self.0.replace_leaf(EPILOGUE_INDEX), self.1.clone()))
		} else {
			self.next_in_parent()
		}
	}

	/// Finds the next step to run afer the prologue of the current pipeline.
	fn after_prologue(self) -> Option<Self> {
		let enclosing_pipeline = self.1.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		if enclosing_pipeline.steps().is_empty() {
			// no steps, go to epilogue.
			self.after_loop()
		} else {
			// this pipeline has steps. Go to the first step entrypoint
			Self(self.0.replace_leaf(STEP0_INDEX), self.1.clone()).enter()
		}
	}

	/// Runs the next step in the parent pipeline.
	fn next_in_parent(self) -> Option<Self> {
		// we're guaranteed that the ancestor is not an epilogue or prologue,
		let ancestor = self.ancestor()?;
		let step_index = ancestor.0.leaf().checked_sub(STEP0_INDEX)?;
		let enclosing_pipeline = ancestor.1.last().expect(
			"StepNavigator should always have at least one enclosing pipeline",
		);

		// is last step in the enclosing pipeline?
		if step_index + 1 >= enclosing_pipeline.steps().len() {
			match ancestor.behavior() {
				Behavior::Loop => ancestor.after_prologue(),
				Behavior::Once => ancestor.after_loop(),
			}
		} else {
			// there are more items in the enclosing pipeline, so we can just
			// increment the step index and return the new navigator to the first
			// executable step in the next item.
			Self(ancestor.0.increment_leaf(), ancestor.1.clone()).enter()
		}
	}

	fn join(self, other: Self) -> Self {
		Self(self.0.concat(other.0), [self.1, other.1].concat())
	}

	fn is_epilogue(&self) -> bool {
		self.0.is_epilogue()
	}

	fn is_prologue(&self) -> bool {
		self.0.is_prologue()
	}
}

#[cfg(test)]
mod test {
	use super::*;

	make_step!(Epilogue1, Static);
	make_step!(Epilogue2, Static);
	make_step!(Epilogue3, Static);

	make_step!(Prologue1, Static);
	make_step!(Prologue2, Static);

	make_step!(Step1, Static);
	make_step!(Step2, Static);
	make_step!(Step3, Static);
	make_step!(Step4, Static);

	make_step!(StepA, Static);
	make_step!(StepB, Static);
	make_step!(StepC, Static);

	make_step!(StepX, Static);
	make_step!(StepY, Static);
	make_step!(StepZ, Static);

	make_step!(StepI, Simulated);
	make_step!(StepII, Simulated);
	make_step!(StepIII, Static);

	#[test]
	fn find_entrypoint() {
		let empty = Pipeline::<EthereumMainnet>::default();
		assert!(StepNavigator::entrypoint(&empty).is_none());

		macro_rules! assert_entrypoint {
			($pipeline:expr, $expected_path:expr, $expected_named:expr) => {{
				let pipeline = $pipeline;
				let StepNavigator(path, pipelines) =
					StepNavigator::entrypoint(&pipeline).unwrap();
				assert_eq!(path, $expected_path);
				assert_eq!(
					pipelines
						.iter()
						.map(|p| p.name().unwrap_or_default())
						.collect::<Vec<&str>>(),
					$expected_named
				);
			}};
		}

		// one step with no prologue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::default().with_step(Step1),
			StepPath::step0(),
			vec![""]
		);

		// one step with prologue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one")
				.with_prologue(Prologue1)
				.with_step(Step1),
			StepPath::prologue(),
			vec!["one"]
		);

		// no steps, but with epilogue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one").with_epilogue(Epilogue1),
			StepPath::epilogue(),
			vec!["one"]
		);

		// one nested step with no prologue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one")
				.with_pipeline(Loop, (Step1,).with_name("two")),
			StepPath::step0().concat(StepPath::step0()),
			vec!["one", "two"]
		);

		// one nested step with prologue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one").with_pipeline(
				Loop,
				(Step1,).with_prologue(Prologue1).with_name("two")
			),
			StepPath::step0().append_prologue(),
			vec!["one", "two"]
		);

		// one nested pipeline with no steps, but with epilogue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one")
				.with_pipeline(Loop, Pipeline::named("two").with_epilogue(Epilogue1)),
			StepPath::step0().append_epilogue(),
			vec!["one", "two"]
		);

		let nested_empty_pipeline = Pipeline::<EthereumMainnet>::default()
			.with_pipeline(Loop, Pipeline::default());
		assert!(StepNavigator::entrypoint(&nested_empty_pipeline).is_none());

		// two levels of nested steps with no prologue
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one").with_pipeline(
				Loop,
				Pipeline::named("two").with_pipeline(Loop, (Step1,).with_name("three"))
			),
			StepPath::step(0).append_step(0).append_step(0),
			vec!["one", "two", "three"]
		);

		// two levels of nested steps with prologue at first level
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one").with_pipeline(
				Loop,
				Pipeline::named("two")
					.with_prologue(Prologue1)
					.with_pipeline(Loop, (Step1,).with_name("three"))
			),
			StepPath::step(0).append_prologue(),
			vec!["one", "two"]
		);

		// two levels of nested steps with prologue at second level
		assert_entrypoint!(
			Pipeline::<EthereumMainnet>::named("one").with_pipeline(
				Loop,
				Pipeline::named("two").with_pipeline(
					Loop,
					(Step1,).with_prologue(Prologue1).with_name("three")
				)
			),
			StepPath::step(0).append_step(0).append_prologue(),
			vec!["one", "two", "three"]
		);
	}

	#[test]
	fn identify_loop_behavior() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3);
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert_eq!(navigator.behavior(), Behavior::Once);

		let pipeline =
			Pipeline::<EthereumMainnet>::default().with_pipeline(Loop, (Step1,));
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert_eq!(navigator.behavior(), Behavior::Loop);

		let pipeline =
			Pipeline::<EthereumMainnet>::default().with_pipeline(Once, (Step1,));
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert_eq!(navigator.behavior(), Behavior::Once);
	}

	#[test]
	fn step_ref_access() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3);
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert!(navigator.step().name().ends_with("Step1"));

		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_prologue(Prologue1)
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3);
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert!(navigator.step().name().ends_with("Prologue1"));

		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_pipeline(Loop, Pipeline::default().with_epilogue(Epilogue1));
		let navigator = StepNavigator::entrypoint(&pipeline).unwrap();
		assert!(navigator.step().name().ends_with("Epilogue1"));
	}

	#[test]
	fn control_flow() {
		let pipeline = Pipeline::<EthereumMainnet>::named("top")
			.with_step(Step1)
			.with_step(Step2)
			.with_pipeline(Loop, |nested: Pipeline<EthereumMainnet>| {
				nested
					.with_step(StepA)
					.with_pipeline(
						Loop,
						(StepX, StepY, StepZ)
							.with_name("nested1.1")
							.with_prologue(Prologue1)
							.with_epilogue(Epilogue1),
					)
					.with_step(StepC)
					.with_pipeline(
						Once,
						(StepI, StepII, StepIII)
							.with_name("nested1.2")
							.with_epilogue(Epilogue2),
					)
					.with_name("nested1")
					.with_prologue(Prologue2)
			})
			.with_step(Step4)
			.with_epilogue(Epilogue3);

		let cursor = StepNavigator::entrypoint(&pipeline).unwrap();
		assert_eq!(cursor.0, StepPath::step0());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(1));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_prologue());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(0));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_prologue());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_step(0));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_step(1));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_step(2));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_step(0));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_step(1));

		let cursor = cursor.next_break().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_epilogue());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(2));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(3).append_step(0));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(3).append_step(1));

		let cursor = cursor.next_break().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(3).append_epilogue());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(0));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_prologue());

		let cursor = cursor.next_break().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(1).append_epilogue());

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::step(2).append_step(2));

		let cursor = cursor.next_break().unwrap();
		assert_eq!(cursor.0, StepPath::step(3));

		let cursor = cursor.next_ok().unwrap();
		assert_eq!(cursor.0, StepPath::epilogue());
	}
}
