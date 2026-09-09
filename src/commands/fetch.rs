use anyhow::{Context, Result, bail};

pub async fn execute(url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .context("failed to send fetch request")?;
    let status = response.status();

    if !status.is_success() {
        bail!("fetch request failed with status {}", status);
    }

    let text = response
        .text()
        .await
        .context("failed to read fetch response body")?;

    let preview: String = text.chars().take(200).collect();
    println!("{}", preview);

    Ok(())
}
