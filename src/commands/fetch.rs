use anyhow::Result;

pub async fn execute(url: &str) -> Result<()> {
    let response = reqwest::get(url).await?;
    let status = response.status();
    if !status.is_success() {
        println!("status: {}", status);
        return Ok(());
    }
    let text = response.text().await?;

    let preview: String = text.chars().take(200).collect();
    println!("{}", preview);

    Ok(())
}
