use anyhow::{Context, Result, bail};

pub async fn execute(url: &str, max_chars: usize) -> Result<()> {
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

    let preview: String = preview_text(&text, max_chars);
    println!("{}", preview);

    Ok(())
}

fn preview_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_text_limits_output_by_character_count() {
        assert_eq!(preview_text("abcdef", 3), "abc");
        assert_eq!(preview_text("你好世界", 2), "你好");
        assert_eq!(preview_text("abc", 10), "abc");
    }
}
