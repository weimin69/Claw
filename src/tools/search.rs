use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

const MAX_ENTRIES: usize = 5000;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_SCAN_BYTES: usize = 8 * 1024 * 1024;
const MAX_MATCHES: usize = 100;
const MAX_TEXT_BYTES: usize = 1024;
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    pattern: String,
    path: String,
}

#[derive(Serialize)]
struct Match {
    path: String,
    line: usize,
    text: String,
    line_truncated: bool,
}

#[derive(Default, Serialize)]
struct SearchOutput {
    matches: Vec<Match>,
    truncated: bool,
    skipped_files: usize,
}

pub(super) fn execute(workspace: &Workspace, input: &str) -> Result<String> {
    let input: SearchInput =
        serde_json::from_str(input).context("invalid search_code tool input")?;
    if input.pattern.is_empty() || input.pattern.len() > MAX_TEXT_BYTES {
        bail!("search_code pattern must contain 1 to 1024 bytes");
    }
    if input.pattern.contains(['\n', '\r']) {
        bail!("search_code pattern must be a single line");
    }
    if input.path.trim().is_empty() {
        bail!("search_code path must not be empty");
    }
    let root = workspace.resolve_existing(Path::new(&input.path))?;
    let mut pending = vec![root]; //未遍历 DFS 避免使用递归
    let mut files = Vec::new(); //找到的普通文件
    let mut visited = 0; //统计以及检查的目录
    let mut output = SearchOutput::default();

    'walk: while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).context("failed to inspect search path")?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let path = workspace.resolve_existing(&path)?;
        if metadata.is_file() {
            files.push(path);
        } else if metadata.is_dir() {
            for entry in fs::read_dir(&path).context("failed to read search directory")? {
                let entry = entry.context("failed to read search directory entry")?;
                if visited == MAX_ENTRIES {
                    output.truncated = true;
                    break 'walk;
                }
                visited += 1;
                let kind = entry
                    .file_type()
                    .context("failed to inspect search entry")?;
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir()
                    && (entry.file_name() == ".git"
                        || entry.file_name() == "target"
                        || entry.file_name() == ".agent-cli")
                {
                    continue;
                }
                if kind.is_file() {
                    files.push(entry.path());
                } else if kind.is_dir() {
                    pending.push(entry.path());
                }
            }
        } else {
            bail!("search_code path must be a regular file or directory");
        }
    }

    files.sort();
    let mut scanned = 0;
    'files: for path in files {
        if scanned == MAX_SCAN_BYTES {
            output.truncated = true;
            break;
        }
        if fs::symlink_metadata(&path)
            .context("failed to inspect search file")?
            .file_type()
            .is_symlink()
        {
            continue;
        }
        let path = workspace.resolve_existing(&path)?;
        let file = fs::File::open(&path).context("failed to open search file")?;
        if !file
            .metadata()
            .context("failed to inspect open search file")?
            .is_file()
        {
            bail!("search_code target is no longer a regular file");
        }
        let remaining = MAX_SCAN_BYTES - scanned;
        let mut bytes = Vec::new();
        file.take(remaining.min(MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .context("failed to read search file")?;
        scanned += bytes.len();
        if bytes.len() > MAX_FILE_BYTES {
            output.skipped_files += 1;
            continue;
        }
        // If the aggregate bound cut this read, omit it rather than reporting
        // partial text as a complete file (or misclassifying a UTF-8 prefix).
        if bytes.len() == remaining {
            output.truncated = true;
            break;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            output.skipped_files += 1;
            continue;
        };
        if bytes.contains(&0) {
            output.skipped_files += 1;
            continue;
        }
        let relative = path
            .strip_prefix(workspace.root())?
            .to_str()
            .context("search_code path is not UTF-8")?;
        for (index, line) in text.lines().enumerate() {
            let Some(position) = line.find(&input.pattern) else {
                continue;
            };
            if output.matches.len() == MAX_MATCHES {
                output.truncated = true;
                break 'files;
            }
            // A long-line excerpt starts at the match so the result always
            // contains the literal pattern, even for matches far into a line.
            let start = if line.len() > MAX_TEXT_BYTES {
                position
            } else {
                0
            };
            let mut end = (start + MAX_TEXT_BYTES).min(line.len());
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            output.matches.push(Match {
                path: relative.to_string(),
                line: index + 1,
                text: line[start..end].to_string(),
                line_truncated: start > 0 || end < line.len(),
            });
            if serde_json::to_string(&output)?.len() > MAX_OUTPUT_BYTES {
                output.matches.pop();
                output.truncated = true;
                break 'files;
            }
        }
    }
    Ok(serde_json::to_string(&output)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::ToolExecutor, tools::BuiltinToolExecutor};

    fn search(workspace: &Workspace, pattern: &str, path: &str) -> serde_json::Value {
        serde_json::from_str(
            &execute(
                workspace,
                &serde_json::json!({
                    "pattern":pattern, "path":path
                })
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn searches_literal_lines_in_sorted_nested_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("z.rs"), "a.b\naXb\na.b a.b\n").unwrap();
        fs::write(directory.path().join("src/a.rs"), "A.B\n你好 a.b\n").unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let value = search(&workspace, "a.b", ".");
        assert_eq!(
            value,
            serde_json::json!({
                "matches":[
                    {"path":"src/a.rs","line":2,"text":"你好 a.b","line_truncated":false},
                    {"path":"z.rs","line":1,"text":"a.b","line_truncated":false},
                    {"path":"z.rs","line":3,"text":"a.b a.b","line_truncated":false}
                ], "truncated":false,"skipped_files":0
            })
        );
        assert_eq!(
            search(&workspace, "a.b", "z.rs")["matches"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            search(&workspace, "absent", ".")["matches"],
            serde_json::json!([])
        );
    }

    #[test]
    fn skips_generated_directories_and_unsupported_files() {
        let directory = tempfile::tempdir().unwrap();
        for name in [".git", "target", ".agent-cli"] {
            fs::create_dir(directory.path().join(name)).unwrap();
            fs::write(directory.path().join(name).join("file"), "needle").unwrap();
        }
        fs::write(directory.path().join("binary"), b"needle\0").unwrap();
        fs::write(directory.path().join("invalid"), [255]).unwrap();
        fs::File::create(directory.path().join("large"))
            .unwrap()
            .set_len((MAX_FILE_BYTES + 1) as u64)
            .unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let value = search(&workspace, "needle", ".");
        assert_eq!(value["matches"], serde_json::json!([]));
        assert_eq!(value["skipped_files"], 3);
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn enforces_match_output_and_utf8_excerpt_limits() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let file = directory.path().join("file");
        fs::write(&file, "needle\n".repeat(MAX_MATCHES)).unwrap();
        assert_eq!(search(&workspace, "needle", "file")["truncated"], false);
        fs::write(&file, "needle\n".repeat(MAX_MATCHES + 1)).unwrap();
        let value = search(&workspace, "needle", "file");
        assert_eq!(value["matches"].as_array().unwrap().len(), MAX_MATCHES);
        assert_eq!(value["truncated"], true);

        let long_line = format!("{}needle{}\n", "你".repeat(500), "界".repeat(500));
        fs::write(&file, long_line.repeat(100)).unwrap();
        let value = search(&workspace, "needle", "file");
        assert!(value.to_string().len() <= MAX_OUTPUT_BYTES);
        assert!(value["matches"].as_array().unwrap().len() < MAX_MATCHES);
        assert_eq!(value["truncated"], true);
        for item in value["matches"].as_array().unwrap() {
            let text = item["text"].as_str().unwrap();
            assert!(text.starts_with("needle"));
            assert!(text.len() <= MAX_TEXT_BYTES);
            assert_eq!(item["line_truncated"], true);
        }
    }

    #[test]
    fn enforces_total_scan_and_directory_entry_limits() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        for index in 0..9 {
            fs::write(
                directory.path().join(format!("{index}.txt")),
                vec![b'x'; MAX_FILE_BYTES],
            )
            .unwrap();
        }
        assert_eq!(search(&workspace, "absent", ".")["truncated"], true);
        let entries = directory.path().join("entries");
        fs::create_dir(&entries).unwrap();
        for index in 0..=MAX_ENTRIES {
            fs::File::create(entries.join(index.to_string())).unwrap();
        }
        assert_eq!(search(&workspace, "absent", "entries")["truncated"], true);
    }

    #[tokio::test]
    async fn returns_recoverable_input_and_path_errors() {
        let directory = tempfile::tempdir().unwrap();
        let mut executor = BuiltinToolExecutor::new(Workspace::new(directory.path()).unwrap());
        for (input, expected) in [
            (serde_json::json!({}), "invalid search_code tool input"),
            (
                serde_json::json!({"pattern":"","path":"."}),
                "pattern must contain",
            ),
            (
                serde_json::json!({"pattern":"x\ny","path":"."}),
                "single line",
            ),
            (
                serde_json::json!({"pattern":"x","path":""}),
                "path must not be empty",
            ),
            (
                serde_json::json!({"pattern":"x","path":"missing"}),
                "failed to resolve",
            ),
            (
                serde_json::json!({"pattern":"x","path":".."}),
                "outside workspace",
            ),
            (
                serde_json::json!({"pattern":"x","path":".","extra":true}),
                "invalid search_code",
            ),
        ] {
            let output = executor
                .execute("search_code", &input.to_string())
                .await
                .unwrap();
            assert!(output.is_error);
            assert!(output.content.contains(expected), "{}", output.content);
        }
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_discovered_symlinks_or_escape_workspace() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), "needle").unwrap();
        fs::write(directory.path().join("real"), "needle").unwrap();
        symlink(outside.path(), directory.path().join("external")).unwrap();
        symlink(
            directory.path().join("real"),
            directory.path().join("internal"),
        )
        .unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let value = search(&workspace, "needle", ".");
        assert_eq!(value["matches"].as_array().unwrap().len(), 1);
        assert_eq!(value["matches"][0]["path"], "real");
        let error = execute(&workspace, r#"{"pattern":"needle","path":"external"}"#).unwrap_err();
        assert!(error.to_string().contains("outside workspace"));
    }
}
