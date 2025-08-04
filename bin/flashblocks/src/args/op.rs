use {
	crate::args::signer::BuilderSigner,
	clap::Parser,
	eyre::{Result, eyre},
	rblib::reth::optimism::{cli::commands::Commands, node::args::RollupArgs},
	std::path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
#[command(next_help_heading = "Rollup")]
pub struct OpRbuilderArgs {
	/// Rollup configuration
	#[command(flatten)]
	pub rollup_args: RollupArgs,

	/// chain block time in milliseconds
	#[arg(
		long = "rollup.chain-block-time",
		default_value = "1000",
		env = "CHAIN_BLOCK_TIME"
	)]
	pub chain_block_time: u64,

	/// Whether to enable revert protection
	#[arg(long = "builder.revert-protection", default_value = "true")]
	pub revert_protection: bool,

	/// Builder secret key for signing last transaction in block
	#[arg(long = "rollup.builder-secret-key", env = "BUILDER_SECRET_KEY")]
	pub builder_signer: Option<BuilderSigner>,

	/// Path to builder playgorund to automatically start up the node connected
	/// to it
	#[arg(
        long = "builder.playground",
        num_args = 0..=1,
        default_missing_value = "$HOME/.playground/devnet/",
        value_parser = expand_path,
        env = "PLAYGROUND_DIR",
    )]
	pub playground: Option<PathBuf>,
}

impl Default for OpRbuilderArgs {
	fn default() -> Self {
		let args = crate::args::Cli::parse_from(["dummy", "node"]);
		let Commands::Node(node_command) = args.command else {
			unreachable!()
		};
		node_command.ext
	}
}

fn expand_path(s: &str) -> Result<PathBuf> {
	shellexpand::full(s)
		.map_err(|e| eyre!("expansion error for `{s}`: {e}"))?
		.into_owned()
		.parse()
		.map_err(|e| eyre!("invalid path after expansion: {e}"))
}
