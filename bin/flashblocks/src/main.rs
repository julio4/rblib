use crate::args::{Cli, CliExt};

mod args;
mod bundle;
mod platform;
mod rpc;

#[cfg(test)]
mod tests;

pub use platform::FlashBlocks;

fn main() -> eyre::Result<()> {
	Cli::parsed()
		.run(|builder, args| async move {
			let node = FlashBlocks::build_node(builder, args);
			let handle = node.launch().await?;
			handle.wait_for_node_exit().await
		})
		.unwrap();

	Ok(())
}
