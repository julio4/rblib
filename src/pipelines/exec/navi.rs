//! Pipeline navigation
//!
//! Types in this module are responsible for navigating through the pipeline
//! steps, They identify next steps, manage loops, and handle nested pipelines,
//! etc.

use {
	crate::{
		pipelines::step::{StepKind, WrappedStep},
		*,
	},
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

	/// Returns the path to the first executable step in the pipeline.
	///
	/// This function will traverse the pipeline and return the innermost
	/// step that is the first in the execution order. If the innermost pipeline
	/// has a prologue step, this will return the path to that step.
	///
	/// Returns `None` if the pipeline is empty or is composed of only empty
	/// pipelines.
	pub fn beginning<P: Platform>(pipeline: &Pipeline<P>) -> Option<Self> {
		if pipeline.is_empty() {
			return None;
		}

		if pipeline.prologue().is_some() {
			// if the pipeline has a prologue, we return the path to it.
			return Some(StepPath::prologue());
		}

		match pipeline.steps.first()? {
			StepOrPipeline::Step(_) => Some(StepPath::first_step()),
			StepOrPipeline::Pipeline(_, pipeline) => {
				Some(StepPath::first_step().concat(StepPath::beginning(pipeline)?))
			}
		}
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
	/// This is useful when doing recursive navigation through the pipeline.
	/// Returns None if the path has only one element.
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
	/// This is useful when you want to navigate to the pipeline or step that
	/// contains the current step or pipeline.
	///
	/// Returns None if the path has only one element.
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

	/// Given a reference to a pipeline, this function will try to locate the
	/// parent pipeline that contains the item pointed to by the current
	/// step path.
	pub fn enclosing<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<(&'a Pipeline<P>, Behavior)> {
		if self.is_toplevel() {
			// Topmost pipelines always have a behavior of `Once`.
			return Some((pipeline, Behavior::Once));
		}

		self
			.clone()
			.remove_leaf()?
			.map_pipeline(pipeline, |p, b| (p, b))
	}

	/// Given a reference to a pipeline, this function will return `true` if
	/// the current step path is the last regular step in the innermost pipeline
	/// that this step path points to.
	///
	/// This does not mean that the step is the last step in the entire pipeline.
	///
	/// Returns `None` if the step path does not point to a valid step or
	/// pipeline or if the step points to an epilogue or prologue step.
	pub fn is_last_in_scope<P: Platform>(
		&self,
		pipeline: &Pipeline<P>,
	) -> Option<bool> {
		let index = self.leaf().checked_sub(STEP0_INDEX)?;

		if self.is_toplevel() {
			// we are at the topmost pipeline, so we can just check if the index
			// is the last step in the pipeline.

			if index >= pipeline.steps().len() {
				// if the index is out of bounds, we cannot be the last step.
				return None;
			}

			return Some(index == pipeline.steps().len() - 1);
		}

		// we're somewhere inside a nested pipeline, so we need to
		// check if the index is the last step in the parent pipeline.
		let (parent, _) = self.enclosing(pipeline)?;

		if index >= parent.steps().len() {
			// if the index is out of bounds, we cannot be the last step.
			return None;
		}

		Some(index == parent.steps().len() - 1)
	}

	/// Given a reference to the top-level pipeline, this function will return the
	/// next step to be executed based on the control flow value returned by the
	/// current step.
	///
	/// Return `None` if there are no further steps to execute in the pipeline
	/// or if the current step path does not point to a valid step or pipeline.
	pub fn advance<P: Platform, K: StepKind>(
		self,
		pipeline: &Pipeline<P>,
		control_flow: &ControlFlow<P, K>,
	) -> Option<NextStep> {
		if control_flow.is_fail() {
			// If the control flow is `Fail`, we cannot continue the pipeline
			// execution. There are no next steps to execute.
			return Some(NextStep::Failure);
		}

		let (_, behavior) = self.enclosing(pipeline)?;
		let is_last_in_scope = self.is_last_in_scope(pipeline)?;

		// if the path points to a step, then we return it as is, otherwise,
		// we find the first executable step in the pipeline pointed to by the path.
		let executable_path = |path: StepPath| {
			match path.map(pipeline, |s| s).expect("bug in step path logic") {
				// item is a step, we can just return the path to it.
				StepOrPipeline::Step(_) => Some(NextStep::Path(path)),
				StepOrPipeline::Pipeline(_, nested) => {
					// item is a nested pipeline, we need to find the
					// first executable step in that pipeline and return the
					// path to it.
					Some(NextStep::Path(path.concat(StepPath::beginning(nested)?)))
				}
			}
		};

		if control_flow.is_ok() {
			if is_last_in_scope {
				match behavior {
					Behavior::Once => match self.next_in_parent_scope(pipeline) {
						Some(next_path) => return executable_path(next_path),
						None => return Some(NextStep::Completed),
					},
					Behavior::Loop => return executable_path(self.first_leaf_in_scope()),
				}
			} else {
				return executable_path(self.next_leaf());
			}
		} else if control_flow.is_continue() {
			match behavior {
				Behavior::Once => {
					// same as ok
					if is_last_in_scope {
						match self.next_in_parent_scope(pipeline) {
							Some(next_path) => return executable_path(next_path),
							None => return Some(NextStep::Completed),
						}
					} else {
						return executable_path(self.clone().next_leaf());
					}
				}
				Behavior::Loop => {
					// rerun the first step in the current sub-pipeline
					return executable_path(self.first_leaf_in_scope());
				}
			}
		} else if control_flow.is_break() {
			// break in a pipeline will stop the execution of the current
			// pipeline and jump to the next step in the parent pipeline.
			match self.next_in_parent_scope(pipeline) {
				Some(next_path) => return executable_path(next_path),
				None => return Some(NextStep::Completed),
			}
		}

		unreachable!("failures already handled earlier")
	}

	/// Given a reference to a pipeline and a function this method will invoke the
	/// function on a step pointed to by the step path.
	///
	/// Returns `None` if the path does not point to a valid step in the pipeline.
	pub fn map_step<P: Platform, R>(
		&self,
		pipeline: &Pipeline<P>,
		f: impl Fn(&Arc<WrappedStep<P>>) -> R,
	) -> Option<R> {
		if pipeline.is_empty() {
			return None;
		}

		if self.is_toplevel() {
			if self.is_prologue() {
				if let Some(prologue) = pipeline.prologue() {
					return Some(f(prologue));
				}

				// pipeline has no prologue
				return None;
			}

			if self.is_epilogue() {
				if let Some(epilogue) = pipeline.epilogue() {
					return Some(f(epilogue));
				}

				// pipeline has no epilogue
				return None;
			}

			if let StepOrPipeline::Step(step) =
				pipeline.steps.get(self.root()?.checked_sub(STEP0_INDEX)?)?
			{
				// If the path points to a step, invoke it.
				return Some(f(step));
			}

			// the path is not pointing to a step.
			return None;
		}

		// we are not the the leaf of the path, keep navigating down the
		// pipeline.
		let Some(StepOrPipeline::Pipeline(_, nested)) =
			pipeline.steps.get(self.root()?.checked_sub(STEP0_INDEX)?)
		else {
			return None;
		};

		self.clone().remove_root()?.map_step(nested, f)
	}

	/// Given a reference to a pipeline, this function will try to locate the
	/// step or nested pipeline that corresponds to the current path.
	///
	/// Returns `None` if the path does not point to a valid item in the pipeline.
	fn map<'a, P: Platform, R>(
		&self,
		pipeline: &'a Pipeline<P>,
		f: impl Fn(&'a StepOrPipeline<P>) -> R,
	) -> Option<R> {
		if pipeline.is_empty() {
			return None;
		}

		let mut path = self.clone();
		let mut item =
			pipeline.steps.get(path.root()?.checked_sub(STEP0_INDEX)?)?;

		while let Some(descendant) = path.remove_root() {
			item = match item {
				StepOrPipeline::Step(_) => return None,
				StepOrPipeline::Pipeline(_, pipeline) => pipeline
					.steps
					.get(descendant.root()?.checked_sub(STEP0_INDEX)?)?,
			};
			path = descendant;
		}

		Some(f(item))
	}

	/// Given a reference to a pipeline, this function will try to locate a
	/// nested pipeline that corresponds to the current path.
	///
	/// Returns `None` if the path does not point to a valid nested pipeline in
	/// the pipeline.
	fn map_pipeline<'a, P: Platform, R>(
		&self,
		pipeline: &'a Pipeline<P>,
		f: impl Fn(&'a Pipeline<P>, Behavior) -> R,
	) -> Option<R> {
		self
			.map(pipeline, |item| match item {
				StepOrPipeline::Step(_) => None,
				StepOrPipeline::Pipeline(behavior, pipeline) => {
					Some(f(pipeline, *behavior))
				}
			})
			.flatten()
	}

	/// Returns a step path that points to the prologue step.
	fn prologue() -> Self {
		Self(smallvec![PROLOGUE_INDEX])
	}

	fn epilogue() -> Self {
		Self(smallvec![EPILOGUE_INDEX])
	}

	/// Returns a new step path with a single element `0`, which represents the
	/// first navigable item in a non-empty pipeline.
	fn first_step() -> Self {
		Self(smallvec![STEP0_INDEX])
	}

	/// Returns a path that points to the next item in the current scope.
	///
	/// This method does not check if the next item is valid or exists in the
	/// pipeline. It simply increments the last index in the path.
	fn next_leaf(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last += 1;
		}
		Self(new_path)
	}

	/// Returns a path that points to the first leaf in the current scope.
	///
	/// This method resets the last index in the path to `0`, effectively
	/// pointing to the first item in the current scope.
	///
	/// This method does not check if the first item is valid or exists in the
	/// pipeline. It simply resets the last index in the path to `0`.
	fn first_leaf_in_scope(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last = 0;
		}
		Self(new_path)
	}

	/// Returns a path that points to the next item in the parent scope(s).
	///
	/// This method will recursively navigate up the pipeline structure in search
	/// of the next item, for example if we are the last step in a nested
	/// pipeline, which in turn is the last step in its parent pipeline, this
	/// method will jump two levels up to the next step in the parent, parent
	/// pipeline and so on recursively until it finds the next item or `None`
	/// if no next item exists.
	///
	/// Used to handle control flows like `break` or last steps in a nested
	/// pipeline.
	fn next_in_parent_scope<P: Platform>(
		self,
		pipeline: &Pipeline<P>,
	) -> Option<Self> {
		let enclosing = self.remove_leaf()?;

		// check if the enclosing pipeline is the last item in its scope
		if enclosing.is_last_in_scope(pipeline)? {
			// enclosing pipeline is the last item in its scope, we will need to
			// navigate one more level up to find the next item.
			return enclosing.next_in_parent_scope(pipeline);
		}

		// otherwise we just need to advance the leaf index on the parent item
		Some(enclosing.next_leaf())
	}
}

#[cfg(test)]
mod test {
	use super::*;

	impl<const N: usize> From<[usize; N]> for StepPath {
		fn from(array: [usize; N]) -> Self {
			Self::new(array.map(|ix| STEP0_INDEX + ix).as_slice())
		}
	}

	make_step!(Epilogue1, Static);
	make_step!(Prologue1, Static);

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
	fn flat_steps_only() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3)
			.with_step(Step4);

		assert!(StepPath::from([90]).map_step(&pipeline, |_| ()).is_none());
		assert_eq!(StepPath::beginning(&pipeline), Some(StepPath::first_step()));

		assert!(StepPath::epilogue().map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::prologue().map_step(&pipeline, |_| ()).is_none());

		assert!(StepPath::from([0]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step1"));
		assert!(StepPath::from([1]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));
		assert!(StepPath::from([2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step3"));

		assert!(StepPath::from([3]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([3])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step4"));

		assert!(StepPath::from([4]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 2]).map_step(&pipeline, |_| ()).is_none());
	}

	#[test]
	fn flat_with_epiprologue() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(Epilogue1)
			.with_prologue(Prologue1)
			.with_step(Step1)
			.with_step(Step2)
			.with_step(Step3)
			.with_step(Step4);
		assert!(StepPath::from([90]).map_step(&pipeline, |_| ()).is_none());
		assert_eq!(StepPath::beginning(&pipeline), Some(StepPath::prologue()));

		assert!(StepPath::beginning(&pipeline)
			.unwrap()
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Prologue1"));

		assert!(StepPath::prologue()
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Prologue1"));

		assert!(StepPath::epilogue()
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Epilogue1"));

		assert!(StepPath::from([0]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step1"));

		assert!(StepPath::from([1]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));

		assert!(StepPath::from([2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step3"));

		assert!(StepPath::from([3]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([3])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step4"));

		assert!(StepPath::from([4]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 2]).map_step(&pipeline, |_| ()).is_none());

		assert!(!StepPath::from([2]).is_last_in_scope(&pipeline).unwrap());
		assert!(StepPath::from([3]).is_last_in_scope(&pipeline).unwrap());
		assert!(StepPath::from([4]).is_last_in_scope(&pipeline).is_none());
	}

	#[test]
	fn nested_steps_only() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_pipeline(Loop, (StepA, StepB, StepC))
			.with_step(Step2);

		assert!(StepPath::epilogue().map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::prologue().map_step(&pipeline, |_| ()).is_none());

		assert!(StepPath::from([99]).map_step(&pipeline, |_| ()).is_none());
		assert_eq!(StepPath::beginning(&pipeline), Some(StepPath::first_step()));
		assert!(StepPath::from([0]).map_step(&pipeline, |_| ()).is_some());

		assert!(StepPath::from([0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step1"));

		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));

		assert!(StepPath::from([1]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 0]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 0, 0])
			.map_step(&pipeline, |_| ())
			.is_none());
		assert!(StepPath::from([1, 0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepA"));

		assert!(StepPath::from([1, 1]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 1])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepB"));

		assert!(StepPath::from([1, 2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepC"));

		assert!(StepPath::from([1, 3]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));

		assert!(StepPath::from([3]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 4]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 0, 1])
			.map_step(&pipeline, |_| ())
			.is_none());
	}

	#[test]
	fn nested_epiprologue() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_pipeline(
				Loop,
				(StepA, StepB, StepC)
					.with_epilogue(Epilogue1)
					.with_prologue(Prologue1),
			)
			.with_step(Step2);

		assert!(StepPath::epilogue().map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::prologue().map_step(&pipeline, |_| ()).is_none());

		assert!(StepPath::from([99]).map_step(&pipeline, |_| ()).is_none());
		assert_eq!(StepPath::beginning(&pipeline), Some(StepPath::first_step()));
		assert!(StepPath::from([0]).map_step(&pipeline, |_| ()).is_some());

		assert!(StepPath::from([0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step1"));

		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));

		assert!(StepPath::from([1]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 0]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 0, 0])
			.map_step(&pipeline, |_| ())
			.is_none());
		assert!(StepPath::from([1, 0])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepA"));

		assert!(StepPath::from([1, 1]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 1])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepB"));

		assert!(StepPath::from([1, 2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([1, 2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("StepC"));

		assert!(StepPath::from([1, 3]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([2]).map_step(&pipeline, |_| ()).is_some());
		assert!(StepPath::from([2])
			.map_step(&pipeline, |x| x.name())
			.unwrap()
			.ends_with("Step2"));

		assert!(StepPath::from([3]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 4]).map_step(&pipeline, |_| ()).is_none());
		assert!(StepPath::from([1, 0, 1])
			.map_step(&pipeline, |_| ())
			.is_none());
	}

	#[test]
	fn next_in_parent_only_steps() {
		let p1 = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_step(Step2)
			.with_pipeline(Loop, |nested: Pipeline<EthereumMainnet>| {
				nested
					.with_step(StepA)
					.with_pipeline(Loop, (StepX, StepY, StepZ))
					.with_step(StepC)
					.with_pipeline(Loop, (StepI, StepII, StepIII))
			})
			.with_step(Step4);

		let step_z = StepPath::from([2, 1, 2]);
		assert!(step_z
			.map_step(&p1, |x| x.name())
			.unwrap()
			.ends_with("StepZ"));

		let step_c = step_z.next_in_parent_scope(&p1).unwrap();
		assert_eq!(step_c, StepPath::from([2, 2]));
		assert!(step_c
			.map_step(&p1, |x| x.name())
			.unwrap()
			.ends_with("StepC"));

		let step_4 = step_c.next_in_parent_scope(&p1).unwrap();
		assert_eq!(step_4, StepPath::from([3]));
		assert!(step_4
			.map_step(&p1, |x| x.name())
			.unwrap()
			.ends_with("Step4"));

		let step_iii = StepPath::from([2, 3, 2]);
		assert!(step_iii
			.map_step(&p1, |x| x.name())
			.unwrap()
			.ends_with("StepIII"));

		let step_4 = step_iii.next_in_parent_scope(&p1).unwrap();
		assert_eq!(step_4, StepPath::from([3]));
		assert!(step_4
			.map_step(&p1, |x| x.name())
			.unwrap()
			.ends_with("Step4"));
		assert_eq!(step_4.next_in_parent_scope(&p1), None);

		let p2 = Pipeline::<EthereumMainnet>::default()
			.with_step(Step1)
			.with_step(Step2)
			.with_pipeline(Loop, |nested: Pipeline<EthereumMainnet>| {
				nested
					.with_step(StepA)
					.with_pipeline(Loop, (StepX, StepY, StepZ))
					.with_step(StepC)
					.with_pipeline(Loop, (StepI, StepII, StepIII))
			});

		let step_iii = StepPath::from([2, 3, 2]);
		assert!(step_iii
			.map_step(&p2, |x| x.name())
			.unwrap()
			.ends_with("StepIII"));

		let none = step_iii.next_in_parent_scope(&p2);
		assert!(
			none.is_none(),
			"StepPath::next_in_parent_scope should return None"
		);
	}
}
