use anyhow::{Result, bail};

pub async fn execute(seconds: u64) -> Result<()> {
    if seconds == 0 || seconds > 60 {
        bail!("seconds must be between 1 and 60");
    }

    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    Ok(())
}
