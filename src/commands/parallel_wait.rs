use anyhow::{Result, bail};
use std::time::Duration;
use tokio::time::sleep;

pub async fn execute(first_seconds: u64, second_seconds: u64) -> Result<()> {
    if first_seconds > 60 || second_seconds > 60 {
        bail!("seconds must be between 1 and 60");
    }
    if first_seconds == 0 || second_seconds == 0 {
        bail!("seconds must be between 1 and 60");
    }
    let first_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(first_seconds)).await;
        first_seconds
    });
    let second_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(second_seconds)).await;
        second_seconds
    });
    let first_seconds = first_handle.await?;
    let second_seconds = second_handle.await?;

    println!("first_seconds: {}", first_seconds);
    println!("second_seconds: {}", second_seconds);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[tokio::test(start_paused = true)]
    async fn completes_within_longest_task_duration() {
        let result = timeout(Duration::from_millis(2500), execute(2, 1))
            .await
            .unwrap();
        assert!(result.is_ok());
    }
}
