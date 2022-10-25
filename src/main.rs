use std::future::pending;
use std::sync::Arc;

use crate::dash::Dash;
use crate::krunner::KRunnerPlugin;

mod dash;
mod krunner;
mod provider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    KRunnerPlugin::new(vec![Arc::new(Dash::new_with_default().await?)], "/krunner").await?;
    pending::<()>().await;
    Ok(())
}
