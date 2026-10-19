use super::MAX_READ_FILE_BYTES;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceTextInput {
    path: String,
    old_text: String,
    new_text: String,
}

/// An in-memory proposal. Original content is retained for the save-time
/// conflict check. Creating a plan does not grant permission to write it.
#[derive(Debug)]
struct EditPlan {
    path: PathBuf,
    original: String,
    updated: String,
    diff: String,
}

pub(super) fn execute(workspace: &Workspace, input: &str) -> Result<String> {
    let input: ReplaceTextInput =
        serde_json::from_str(input).context("invalid replace_text tool input")?;
    let plan = prepare_edit(workspace, input)?;
    let path = plan
        .path
        .strip_prefix(workspace.root())?
        .to_str()
        .context("replace_text path is not UTF-8")?;
    // Prepare a bounded result before committing, so result construction cannot
    // turn an applied edit into a reported tool failure.
    let mut diff = plan.diff.as_str();
    let result = loop {
        let result = serde_json::json!({
            "path":path,"applied":true,"changed":plan.original != plan.updated,
            "diff":diff,"diff_truncated":diff.len() < plan.diff.len()
        })
        .to_string();
        if result.len() <= 32 * 1024 {
            break result;
        }
        if diff.is_empty() {
            bail!("replace_text result path exceeds output limit");
        }
        let mut end = diff.len() / 2;
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        diff = &diff[..end];
    };
    apply_edit(workspace, &plan)?;
    Ok(result)
}

/// Prepare a proposal only; never writes to disk or sends content to the model.
fn prepare_edit(workspace: &Workspace, input: ReplaceTextInput) -> Result<EditPlan> {
    if input.path.trim().is_empty() {
        bail!("path must not be empty");
    }
    let path = workspace.resolve_existing(Path::new(&input.path))?;
    if !path.is_file() {
        bail!("replace_text path must be a regular file");
    }
    let file = File::open(&path).context("failed to open replace_text path")?;
    if !file
        .metadata()
        .context("failed to inspect replace_text path")?
        .is_file()
    {
        bail!("replace_text path must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take((MAX_READ_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("failed to read replace_text file")?;
    if bytes.len() > MAX_READ_FILE_BYTES {
        bail!("replace_text file exceeds 1 MiB limit");
    }
    if bytes.contains(&0) || input.new_text.contains('\0') {
        bail!("replace_text does not support NUL bytes");
    }
    let content = std::str::from_utf8(&bytes).context("replace_text requires UTF-8 text")?;
    if input.old_text.is_empty() && !content.is_empty() {
        bail!("old_text must not be empty for a nonempty file");
    }
    if !input.old_text.is_empty() {
        let position = content
            .find(&input.old_text)
            .context("old_text was not found in file")?;
        // Check overlapping matches too: "aa" in "aaa" is ambiguous.
        let next = position
            + input
                .old_text
                .chars()
                .next()
                .context("old_text must not be empty")?
                .len_utf8();
        if content[next..].contains(&input.old_text) {
            bail!("old_text matches multiple locations; provide more surrounding context");
        }
    }
    let retained = content.len() - input.old_text.len();
    if input.new_text.len() > MAX_READ_FILE_BYTES - retained {
        bail!("replace_text result exceeds 1 MiB limit");
    }
    let updated = content.replacen(&input.old_text, &input.new_text, 1);
    let relative = path
        .strip_prefix(workspace.root())?
        .to_str()
        .context("replace_text path is not UTF-8")?;
    let diff = replacement_diff(relative, content, &updated);
    Ok(EditPlan {
        path,
        original: content.to_string(),
        updated,
        diff,
    })
}

/// Save a prepared edit using a same-directory temporary file. The caller must
/// authorize writes before calling this function. Checks detect stale content
/// but do not provide isolation against concurrent filesystem changes.
fn apply_edit(workspace: &Workspace, plan: &EditPlan) -> Result<()> {
    let permissions = validate_edit_target(workspace, plan)?;
    let parent = plan
        .path
        .parent()
        .context("edit target has no parent directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("failed to create temporary edit file")?;
    temporary
        .write_all(plan.updated.as_bytes())
        .context("failed to write temporary edit file")?;
    temporary
        .as_file()
        .set_permissions(permissions.clone())
        .context("failed to preserve edit target permissions")?;
    temporary
        .as_file()
        .sync_all()
        .context("failed to sync temporary edit file")?;

    // Recheck after preparing the temporary file, as close to rename as possible.
    let current_permissions = validate_edit_target(workspace, plan)?;
    if current_permissions != permissions {
        bail!("edit target permissions changed; prepare a new edit");
    }
    temporary
        .persist(&plan.path)
        .map_err(|error| error.error)
        .context("failed to replace edit target")?;
    Ok(())
}

fn validate_edit_target(workspace: &Workspace, plan: &EditPlan) -> Result<std::fs::Permissions> {
    let resolved = workspace.resolve_existing(&plan.path)?;
    if resolved != plan.path {
        bail!("edit target path changed; prepare a new edit");
    }
    let metadata =
        std::fs::symlink_metadata(&plan.path).context("failed to inspect edit target")?;
    if !metadata.is_file() {
        bail!("edit target must remain a regular file");
    }
    let file = File::open(&plan.path).context("failed to reopen edit target")?;
    let metadata = file
        .metadata()
        .context("failed to inspect open edit target")?;
    if !metadata.is_file() {
        bail!("edit target must remain a regular file");
    }
    if metadata.permissions().readonly() {
        bail!("edit target is read-only");
    }
    let mut content = Vec::new();
    file.take((MAX_READ_FILE_BYTES + 1) as u64)
        .read_to_end(&mut content)
        .context("failed to re-read edit target")?;
    if content != plan.original.as_bytes() {
        bail!("edit target content changed; read the file and prepare a new edit");
    }
    Ok(metadata.permissions())
}

/// A single replacement needs only one hunk, with three unchanged context
/// lines on either side. Comparing common ends avoids quadratic diff work.
fn replacement_diff(path: &str, original: &str, updated: &str) -> String {
    if original == updated {
        return String::new();
    }
    let old: Vec<_> = original.split_inclusive('\n').collect();
    let new: Vec<_> = updated.split_inclusive('\n').collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = prefix.saturating_sub(3);
    let old_change_end = old.len() - suffix;
    let new_change_end = new.len() - suffix;
    let context = suffix.min(3);
    let old_count = old_change_end + context - start;
    let new_count = new_change_end + context - start;
    let old_start = start + usize::from(old_count > 0);
    let new_start = start + usize::from(new_count > 0);
    // Debug quoting prevents unusual names from injecting extra header lines.
    let mut diff = format!(
        "--- {:?}\n+++ {:?}\n@@ -{old_start},{old_count} +{new_start},{new_count} @@\n",
        format!("a/{path}"),
        format!("b/{path}")
    );
    for line in &old[start..prefix] {
        push_diff_line(&mut diff, ' ', line);
    }
    for line in &old[prefix..old_change_end] {
        push_diff_line(&mut diff, '-', line);
    }
    for line in &new[prefix..new_change_end] {
        push_diff_line(&mut diff, '+', line);
    }
    for line in &old[old_change_end..old_change_end + context] {
        push_diff_line(&mut diff, ' ', line);
    }
    diff
}

fn push_diff_line(diff: &mut String, prefix: char, line: &str) {
    diff.push(prefix);
    diff.push_str(line);
    if !line.ends_with('\n') {
        diff.push_str("\n\\ No newline at end of file\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn execute_bounds_diff_without_losing_the_applied_result() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        let old = "a".repeat(40_000);
        let new = "b".repeat(40_000);
        fs::write(&path, &old).unwrap();
        let output = execute(
            &workspace,
            &serde_json::json!({"path":"file","old_text":old,"new_text":new}).to_string(),
        )
        .unwrap();
        assert!(output.len() <= 32 * 1024);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["applied"], true);
        assert_eq!(value["diff_truncated"], true);
        assert_eq!(fs::read_to_string(path).unwrap(), new);
    }

    fn input(old: &str, new: &str) -> ReplaceTextInput {
        ReplaceTextInput {
            path: "file".to_string(),
            old_text: old.to_string(),
            new_text: new.to_string(),
        }
    }

    #[test]
    fn initializes_existing_empty_file_and_preserves_conflict_check() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "").unwrap();
        let plan = prepare_edit(&workspace, input("", "你好\n")).unwrap();
        assert_eq!(plan.original, "");
        assert_eq!(plan.updated, "你好\n");
        assert!(plan.diff.contains("@@ -0,0 +1,1 @@"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        apply_edit(&workspace, &plan).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "你好\n");

        fs::write(&path, "").unwrap();
        let plan = prepare_edit(&workspace, input("", "agent content")).unwrap();
        fs::write(&path, "user content").unwrap();
        assert!(apply_edit(&workspace, &plan).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "user content");
    }

    #[test]
    fn empty_initialization_obeys_content_limits_and_nonempty_rules() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "").unwrap();
        let plan = prepare_edit(&workspace, input("", "")).unwrap();
        assert!(plan.updated.is_empty());
        assert!(plan.diff.is_empty());
        assert!(prepare_edit(&workspace, input("missing", "text")).is_err());
        assert!(prepare_edit(&workspace, input("", "\0")).is_err());
        assert!(prepare_edit(&workspace, input("", &"x".repeat(MAX_READ_FILE_BYTES + 1))).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        for content in ["text", " \n"] {
            fs::write(&path, content).unwrap();
            let error = prepare_edit(&workspace, input("", "overwrite")).unwrap_err();
            assert!(error.to_string().contains("nonempty file"));
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
    }

    #[test]
    fn applies_edit_and_cleans_up_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "before\r\n你好\r\nafter\r\n").unwrap();
        let plan = prepare_edit(&workspace, input("你好", "世界")).unwrap();
        apply_edit(&workspace, &plan).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), plan.updated);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_stale_and_removed_targets_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "original").unwrap();
        let plan = prepare_edit(&workspace, input("original", "updated")).unwrap();
        fs::write(&path, "user changes").unwrap();
        assert!(
            apply_edit(&workspace, &plan)
                .unwrap_err()
                .to_string()
                .contains("content changed")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "user changes");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        fs::remove_file(&path).unwrap();
        assert!(apply_edit(&workspace, &plan).is_err());
        assert!(!path.exists());
        fs::create_dir(&path).unwrap();
        assert!(
            apply_edit(&workspace, &plan)
                .unwrap_err()
                .to_string()
                .contains("regular file")
        );
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_file_permissions_and_refuses_read_only_targets() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "original").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
        let plan = prepare_edit(&workspace, input("original", "updated")).unwrap();
        apply_edit(&workspace, &plan).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o750
        );
        let plan = prepare_edit(&workspace, input("updated", "again")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o440)).unwrap();
        assert!(
            apply_edit(&workspace, &plan)
                .unwrap_err()
                .to_string()
                .contains("read-only")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "updated");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_retargeted_symlinks_at_save_time() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "original").unwrap();
        let plan = prepare_edit(&workspace, input("original", "updated")).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "original").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(outside.path(), &path).unwrap();
        assert!(
            apply_edit(&workspace, &plan)
                .unwrap_err()
                .to_string()
                .contains("outside workspace")
        );
        assert_eq!(fs::read_to_string(outside.path()).unwrap(), "original");
        fs::remove_file(&path).unwrap();
        let other = directory.path().join("other");
        fs::write(&other, "original").unwrap();
        symlink(&other, &path).unwrap();
        assert!(
            apply_edit(&workspace, &plan)
                .unwrap_err()
                .to_string()
                .contains("path changed")
        );
        assert_eq!(fs::read_to_string(other).unwrap(), "original");
    }

    #[test]
    fn diff_handles_line_insertions_deletions_and_missing_newlines() {
        assert_eq!(
            replacement_diff("file", "a\nb\n", "a\nx\ny\n"),
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -1,2 +1,3 @@\n a\n-b\n+x\n+y\n"
        );
        assert_eq!(
            replacement_diff("file", "a\n", ""),
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -1,1 +0,0 @@\n-a\n"
        );
        assert_eq!(
            replacement_diff("file", "", "a\n"),
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -0,0 +1,1 @@\n+a\n"
        );
        assert_eq!(
            replacement_diff("file", "a", "b\n"),
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -1,1 +1,1 @@\n-a\n\\ No newline at end of file\n+b\n"
        );
        assert!(replacement_diff("file", "same", "same").is_empty());
    }

    #[test]
    fn diff_uses_bounded_context_and_correct_line_numbers() {
        let original = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let updated = original.replace("5\n", "five\n");
        assert_eq!(
            replacement_diff("file", original, &updated),
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n"
        );
    }

    #[test]
    fn prepares_unique_unicode_replacement_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        let original = "before\r\n你好\r\nafter\r\n";
        fs::write(&path, original).unwrap();
        let plan = prepare_edit(&workspace, input("你好", "世界")).unwrap();
        assert_eq!(plan.path, path.canonicalize().unwrap());
        assert_eq!(plan.original, original);
        assert_eq!(plan.updated, "before\r\n世界\r\nafter\r\n");
        assert_eq!(
            plan.diff,
            "--- \"a/file\"\n+++ \"b/file\"\n@@ -1,3 +1,3 @@\n before\r\n-你好\r\n+世界\r\n after\r\n"
        );
        assert_eq!(
            prepare_edit(&workspace, input("你好\r\n", ""))
                .unwrap()
                .updated,
            "before\r\nafter\r\n"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn rejects_missing_empty_duplicate_and_overlapping_matches() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        for (content, old, message) in [
            ("hello", "", "must not be empty"),
            ("hello", "absent", "was not found"),
            ("a a", "a", "multiple locations"),
            ("aaa", "aa", "multiple locations"),
            ("你好你好你", "你好你", "multiple locations"),
        ] {
            fs::write(&path, content).unwrap();
            let error = prepare_edit(&workspace, input(old, "x")).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
    }

    #[test]
    fn rejects_invalid_paths_and_unsupported_files() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        for (target, message) in [
            ("", "path must not be empty"),
            ("missing", "failed to resolve workspace path"),
            (".", "regular file"),
            ("..", "outside workspace"),
        ] {
            let mut request = input("a", "b");
            request.path = target.to_string();
            let error = prepare_edit(&workspace, request).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
        for (bytes, message) in [
            (vec![255], "UTF-8"),
            (vec![0], "NUL bytes"),
            (vec![b'a'; MAX_READ_FILE_BYTES + 1], "1 MiB"),
        ] {
            fs::write(&path, &bytes).unwrap();
            let error = prepare_edit(&workspace, input("a", "b")).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn limits_replacement_result_and_rejects_binary_output() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "a").unwrap();
        let error =
            prepare_edit(&workspace, input("a", &"b".repeat(MAX_READ_FILE_BYTES + 1))).unwrap_err();
        assert!(error.to_string().contains("result exceeds"));
        assert!(prepare_edit(&workspace, input("a", "\0")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "a");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_to_outside_file() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "a").unwrap();
        std::os::unix::fs::symlink(outside.path(), directory.path().join("file")).unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let error = prepare_edit(&workspace, input("a", "b")).unwrap_err();
        assert!(error.to_string().contains("outside workspace"));
        assert_eq!(fs::read_to_string(outside.path()).unwrap(), "a");
    }
}
