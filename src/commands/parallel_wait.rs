use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::sleep;

pub async fn execute(first_seconds: u64, second_seconds: u64) -> Result<()> {
    if first_seconds > 60 || first_seconds == 0 {
        bail!("first_seconds must be between 1 and 60");
    }
    if second_seconds > 60 || second_seconds == 0 {
        bail!("second_seconds must be between 1 and 60");
    }
    let first_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(first_seconds)).await;
        Ok::<u64, anyhow::Error>(first_seconds)
    });
    let second_handle = tokio::spawn(async move {
        sleep(Duration::from_secs(second_seconds)).await;
        Ok::<u64, anyhow::Error>(second_seconds)
    });
    let (first_seconds, second_seconds) = wait_for_both(first_handle, second_handle).await?;

    println!("first_seconds: {}", first_seconds);
    println!("second_seconds: {}", second_seconds);
    Ok(())
}

// waits for both tasks to complete and returns their results
async fn wait_for_both(
    first_handle: JoinHandle<Result<u64>>,
    second_handle: JoinHandle<Result<u64>>,
) -> Result<(u64, u64)> {
    let first_join_result = first_handle.await;
    let second_join_result = second_handle.await;

    let first_result = first_join_result.context("first task failed to join")?;
    let second_result = second_join_result.context("second task failed to join")?;

    let first_seconds = first_result.context("first task returned a business error")?;
    let second_seconds = second_result.context("second task returned a business error")?;

    Ok((first_seconds, second_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Instant, timeout};

    #[tokio::test(start_paused = true)]
    async fn completes_within_longest_task_duration() {
        let result = timeout(Duration::from_millis(2500), execute(2, 1))
            .await
            .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn business_failure_is_returned_inside_join_result() {
        let handle = tokio::spawn(async {
            Err::<u64, anyhow::Error>(anyhow::anyhow!("task business failure"))
        });

        let out_result = handle.await;
        assert!(out_result.is_ok());

        let inner_result = out_result.unwrap();
        assert!(inner_result.is_err());
    }
    #[tokio::test]
    async fn panic_is_returned_as_join_error() {
        let handle = tokio::spawn(async { panic!("task panic!") });

        let out_result = handle.await;
        assert!(out_result.is_err());

        let join_error = out_result.unwrap_err();
        assert!(join_error.is_panic());
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_second_task_when_first_panics() {
        let first_handle: JoinHandle<Result<u64>> =
            tokio::spawn(async { panic!("first task panic") });
        let second_handle = tokio::spawn(async {
            sleep(Duration::from_secs(2)).await;
            Ok::<u64, anyhow::Error>(2)
        });

        let started_at = Instant::now();

        let result = wait_for_both(first_handle, second_handle).await;
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "first task failed to join");
        assert_eq!(started_at.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_second_task_when_first_returns_business_error() {
        let first_handle: JoinHandle<Result<u64>> = tokio::spawn(async {
            Err::<u64, anyhow::Error>(anyhow::anyhow!("first task business failure"))
        });
        let second_handle = tokio::spawn(async {
            sleep(Duration::from_secs(2)).await;
            Ok::<u64, anyhow::Error>(2)
        });

        let started_at = Instant::now();

        let result = wait_for_both(first_handle, second_handle).await;
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "first task returned a business error");
        assert_eq!(started_at.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test]
    async fn second_panic_has_join_context() {
        let first_handle: JoinHandle<Result<u64>> = tokio::spawn(async { Ok(1) });
        let second_handle: JoinHandle<Result<u64>> =
            tokio::spawn(async { panic!("second task panic") });

        let result = wait_for_both(first_handle, second_handle).await;
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "second task failed to join");
    }

    #[tokio::test]
    async fn second_business_failure_has_context() {
        let first_handle: JoinHandle<Result<u64>> = tokio::spawn(async { Ok(1) });
        let second_handle: JoinHandle<Result<u64>> = tokio::spawn(async {
            Err::<u64, anyhow::Error>(anyhow::anyhow!("second task business failure"))
        });

        let result = wait_for_both(first_handle, second_handle).await;
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "second task returned a business error");
    }

    #[tokio::test]
    async fn join_error_takes_priority_over_first_business_error() {
        let first_handle: JoinHandle<Result<u64>> = tokio::spawn(async {
            Err::<u64, anyhow::Error>(anyhow::anyhow!("first task business failure"))
        });
        let second_handle: JoinHandle<Result<u64>> =
            tokio::spawn(async { panic!("second task panic") });

        let result = wait_for_both(first_handle, second_handle).await;
        let error = result.unwrap_err();
        assert_eq!(error.to_string(), "second task failed to join");
    }
}
