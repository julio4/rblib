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

impl StepPath {
	/// Constructs a new step path from a list of indices.
	pub fn new(path: impl Into<SmallVec<[usize; 8]>>) -> Self {
		let path: SmallVec<[usize; 8]> = path.into();
		assert!(!path.is_empty(), "StepPath cannot be empty");
		Self(path)
	}

	/// Returns a new step path with a single element `0`, which represents the
	/// first navigable item in a non-empty pipeline.
	pub fn zero() -> Self {
		Self(smallvec![0])
	}

	/// Returns the number of elements in the path.
	pub fn len(&self) -> usize {
		self.0.len()
	}

	/// Returns the path to the first step in the pipeline.
	///
	/// This function will traverse the pipeline and return the innermost
	/// step that is the first in the execution order.
	///
	/// Returns `None` if the pipeline is empty or is composed of only empty
	/// pipelines.
	pub fn first_executable_step<P: Platform>(
		pipeline: &Pipeline<P>,
	) -> Option<Self> {
		if pipeline.is_empty() {
			return None;
		}

		match pipeline.steps.first()? {
			StepOrPipeline::Step(_) => Some(StepPath::zero()),
			StepOrPipeline::Pipeline(_, pipeline) => {
				Some(StepPath::zero().join(StepPath::first_executable_step(pipeline)?))
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
		if self.0.len() < 2 {
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
		if self.0.len() < 2 {
			None
		} else {
			Some(StepPath(self.0[..self.0.len() - 1].into()))
		}
	}

	/// Appends a new step path to the current path.
	pub fn join(self, other: Self) -> Self {
		let mut new_path = self.0;
		new_path.extend(other.0);
		Self(new_path)
	}

	/// Given a reference to a pipeline, this function will try to locate the
	/// parent pipeline that contains the item pointed to by the current
	/// step path.
	pub fn parent<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<(&'a Pipeline<P>, Behavior)> {
		if self.len() == 1 {
			// If the path has only one element, the the topmost pipeline is the
			// one that contains the step or nested pipeline. Topmost pipelines always
			// have a behavior of `Once`.
			return Some((pipeline, Behavior::Once));
		}

		self.clone().remove_leaf()?.locate_pipeline(pipeline)
	}

	/// Given a reference to a pipeline, this function will return `true` if
	/// the current step path is the last step in the innermost pipeline that
	/// this step path points to.
	///
	/// This does not mean that the step is the last step in the entire pipeline.
	///
	/// Returns `None` if the step path does not point to a valid step or
	/// pipeline.
	pub fn is_last_in_scope<P: Platform>(
		&self,
		pipeline: &Pipeline<P>,
	) -> Option<bool> {
		let index = self.leaf();

		if self.len() == 1 {
			// we are at the topmost pipeline, so we can just check if the index
			// is the last step in the pipeline.
			return Some(index == pipeline.steps().len() - 1);
		}

		// we're somewhere inside a nested pipeline, so we need to
		// check if the index is the last step in the parent pipeline.
		let (parent, _) = self.parent(pipeline)?;
		Some(index == parent.steps().len() - 1)
	}

	/// Given a reference to a pipeline, this function will return the next step
	/// to be executed based on the control flow value returned by the current
	/// step.
	///
	/// Return `None` if there are no further steps to execute in the pipeline
	/// or if the current step path does not point to a valid step or pipeline.
	pub fn next_step<P: Platform, K: StepKind>(
		&self,
		pipeline: &Pipeline<P>,
		control_flow: &ControlFlow<P, K>,
	) -> Option<NextStep> {
		if control_flow.is_fail() {
			// If the control flow is `Fail`, we cannot continue the pipeline
			// execution. There are no next steps to execute.
			return Some(NextStep::Failure);
		}

		let (parent, behavior) = self.parent(pipeline)?;
		let is_last_in_scope = self.is_last_in_scope(pipeline)?;

		// if the path points to a step, then we return it as is, otherwise,
		// we find the first executable step in the pipeline pointed to by the path.
		let executable_path = |path: StepPath| {
			match path.locate(pipeline).expect("bug in step path logic") {
				// item is a step, we can just return the path to it.
				StepOrPipeline::Step(_) => Some(NextStep::Path(path)),
				StepOrPipeline::Pipeline(_, nested) => {
					// item is a nested pipeline, we need to find the
					// first executable step in that pipeline and return the
					// path to it.
					Some(NextStep::Path(
						path.join(StepPath::first_executable_step(nested)?),
					))
				}
			}
		};

		if control_flow.is_ok() {
			if is_last_in_scope {
				match behavior {
					Behavior::Once => {
						// if we are the last step in the topmost pipeline, we can
						// complete the pipeline execution.
						if core::ptr::eq(parent, pipeline) {
							return Some(NextStep::Completed);
						} else {
							// otherwise we need to go to the next step in the parent
							// pipeline.
							todo!(
								"Ok/last step in Once sub-pipeline, but not topmost pipeline"
							);
						}
					}
					Behavior::Loop => return executable_path(self.clone()),
				}
			} else {
				return executable_path(self.clone().next_leaf());
			}
		} else if control_flow.is_continue() {
			match behavior {
				Behavior::Once => {
					// same as ok
					if is_last_in_scope {
						// terminate current sub-pipeline
						todo!("Continue/last step in Once sub-pipeline");
					} else {
						// same as ok
						return executable_path(self.clone().next_leaf());
					}
				}
				Behavior::Loop => {
					// rerun the first step in the current sub-pipeline
					return executable_path(self.clone().first_leaf_in_scope());
				}
			}
		} else if control_flow.is_break() {
			todo!("ControlFlowKind::Break not implemented yet");
		}

		unreachable!("failures already handled earlier")
	}

	/// Given a reference to a pipeline, this function will try to locate the
	/// step or nested pipeline that corresponds to the current path.
	///
	/// Returns `None` if the path does not point to a valid item in the pipeline.
	pub fn locate<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<&'a StepOrPipeline<P>> {
		if pipeline.is_empty() {
			return None;
		}

		let mut path = self.clone();
		let mut item = pipeline.steps.get(path.root()?)?;

		while let Some(descendant) = path.remove_root() {
			item = match item {
				StepOrPipeline::Step(_) => return None,
				StepOrPipeline::Pipeline(_, pipeline) => {
					pipeline.steps.get(descendant.root()?)?
				}
			};
			path = descendant;
		}

		Some(item)
	}

	pub fn locate_step<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<&'a Arc<WrappedStep<P>>> {
		self.locate(pipeline).and_then(|item| match item {
			StepOrPipeline::Step(step) => Some(step),
			StepOrPipeline::Pipeline(_, _) => None,
		})
	}

	fn locate_pipeline<'a, P: Platform>(
		&self,
		pipeline: &'a Pipeline<P>,
	) -> Option<(&'a Pipeline<P>, Behavior)> {
		self.locate(pipeline).and_then(|item| match item {
			StepOrPipeline::Step(_) => None,
			StepOrPipeline::Pipeline(behavior, pipeline) => {
				Some((pipeline, *behavior))
			}
		})
	}

	fn next_leaf(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last += 1;
		}
		Self(new_path)
	}

	fn first_leaf_in_scope(self) -> Self {
		let mut new_path = self.0;
		if let Some(last) = new_path.last_mut() {
			*last = 0;
		}
		Self(new_path)
	}
}

impl<const N: usize> From<[usize; N]> for StepPath {
	fn from(array: [usize; N]) -> Self {
		Self::new(array.as_slice())
	}
}

#[cfg(test)]
mod test {
	use super::{steps::*, *};

	#[test]
	fn step_by_path_flat() {
		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(BuilderEpilogue)
			.with_step(GatherBestTransactions)
			.with_step(PriorityFeeOrdering)
			.with_step(TotalProfitOrdering)
			.with_step(RevertProtection);

		assert!(StepPath::from([90]).locate_step(&pipeline).is_none());
		assert_eq!(
			StepPath::first_executable_step(&pipeline),
			Some(StepPath::zero())
		);

		assert!(StepPath::from([0]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([0])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(StepPath::from([1]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([1])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(StepPath::from([2]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([2])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"));

		assert!(StepPath::from([3]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([3])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("RevertProtection"));

		assert!(StepPath::from([4]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([1, 2]).locate_step(&pipeline).is_none());
	}

	#[test]
	fn step_by_path_nested() {
		pub struct TestStep;
		impl<P: Platform> Step<P> for TestStep {
			type Kind = Simulated;

			async fn step(
				self: Arc<Self>,
				_payload: SimulatedPayload<P>,
				_ctx: StepContext<P>,
			) -> ControlFlow<P, Simulated> {
				todo!()
			}
		}

		let pipeline = Pipeline::<EthereumMainnet>::default()
			.with_epilogue(BuilderEpilogue)
			.with_step(TestStep)
			.with_pipeline(
				Loop,
				(
					GatherBestTransactions,
					PriorityFeeOrdering,
					TotalProfitOrdering,
				),
			)
			.with_step(RevertProtection);

		assert!(StepPath::from([99]).locate_step(&pipeline).is_none());
		assert_eq!(
			StepPath::first_executable_step(&pipeline),
			Some(StepPath::zero())
		);
		assert!(StepPath::from([0]).locate_step(&pipeline).is_some());

		assert!(StepPath::from([0])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TestStep"));

		assert!(StepPath::from([1]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([1, 0, 0]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("GatherBestTransactions"));
		assert!(StepPath::from([1, 1]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([1, 1])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("PriorityFeeOrdering"));
		assert!(StepPath::from([1, 2]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([1, 2])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("TotalProfitOrdering"));
		assert!(StepPath::from([1, 3]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([2]).locate_step(&pipeline).is_some());
		assert!(StepPath::from([2])
			.locate_step(&pipeline)
			.unwrap()
			.name()
			.ends_with("RevertProtection"));
		assert!(StepPath::from([3]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([1, 4]).locate_step(&pipeline).is_none());
		assert!(StepPath::from([1, 0, 1]).locate_step(&pipeline).is_none());
	}
}
