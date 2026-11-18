use crate::convert::BatchResult;

/// Fire a desktop notification (swaync on this setup). Never panics; failures
/// are swallowed — a missing notification daemon must not break a conversion.
pub fn send(enabled: bool, summary: &str, body: &str) {
    if !enabled {
        return;
    }
    let _ = notify_rust::Notification::new()
        .appname("Mandarine")
        .summary(summary)
        .body(body)
        .timeout(notify_rust::Timeout::Milliseconds(6000))
        .show();
}

/// Fire a notification with action buttons for ML4W / SwayNC integration.
/// `open_folder` points to the directory containing the converted output so
/// the user can jump straight to it; `copy_path` holds a path to copy to the
/// clipboard.
pub fn send_with_actions(
    enabled: bool,
    summary: &str,
    body: &str,
    open_folder: Option<&std::path::PathBuf>,
    copy_path: Option<&std::path::PathBuf>,
) {
    if !enabled {
        return;
    }
    let mut n = notify_rust::Notification::new();
    n.appname("Mandarine");
    n.summary(summary);
    n.body(body);
    n.timeout(notify_rust::Timeout::Milliseconds(8000));

    if open_folder.is_some() {
        n.action("open-folder", "Open Folder");
    }
    if copy_path.is_some() {
        n.action("copy-path", "Copy Path");
    }

    let _ = n.show();
}

pub fn job_started(enabled: bool, total: usize) {
    send(enabled, "Mandarine", &format!("Converting {total} file{}…", if total == 1 { "" } else { "s" }));
}

pub fn summarize(enabled: bool, result: &BatchResult, label: &str) {
    let ok = result.ok_count();
    let failed = result.failed_count();
    let summary = match (ok, failed) {
        (n, 0) => format!("Mandarine: {n} file{} converted", if n == 1 { "" } else { "s" }),
        (0, n) => format!("Mandarine: {n} job{} failed", if n == 1 { "" } else { "s" }),
        (o, f) => format!("Mandarine: {o} ok, {f} failed"),
    };
    let mut body_lines: Vec<String> = vec![];
    for outcome in &result.outcomes {
        match (&outcome.outputs, &outcome.error) {
            (outputs, None) if !outputs.is_empty() => {
                let names: Vec<String> = outputs
                    .iter()
                    .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()))
                    .collect();
                body_lines.push(format!("✅ {}", names.join(", ")));
            }
            (_, Some(e)) => body_lines.push(format!("⚠️ {}", short_error(e))),
            _ => {}
        }
        if body_lines.len() >= 6 {
            break;
        }
    }
    if body_lines.is_empty() {
        body_lines.push(label.to_string());
    }
    send(enabled, &summary, &body_lines.join("\n"));
}

/// Summarize with action buttons: "Open Folder" opens the directory of the
/// first output; "Copy Path" copies its path to the clipboard.
pub fn summarize_with_actions(enabled: bool, result: &BatchResult) {
    if !enabled {
        return;
    }
    let ok = result.ok_count();
    let failed = result.failed_count();
    let summary = match (ok, failed) {
        (n, 0) => format!("Mandarine: {n} file{} converted", if n == 1 { "" } else { "s" }),
        (0, n) => format!("Mandarine: {n} job{} failed", if n == 1 { "" } else { "s" }),
        (o, f) => format!("Mandarine: {o} ok, {f} failed"),
    };
    let mut body_lines: Vec<String> = vec![];
    let mut first_output: Option<std::path::PathBuf> = None;
    for outcome in &result.outcomes {
        match (&outcome.outputs, &outcome.error) {
            (outputs, None) if !outputs.is_empty() => {
                let names: Vec<String> = outputs
                    .iter()
                    .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()))
                    .collect();
                body_lines.push(format!("✅ {}", names.join(", ")));
                if first_output.is_none() {
                    first_output = outputs.first().cloned();
                }
            }
            (_, Some(e)) => body_lines.push(format!("⚠️ {}", short_error(e))),
            _ => {}
        }
        if body_lines.len() >= 6 {
            break;
        }
    }
    if body_lines.is_empty() {
        body_lines.push("Finished".to_string());
    }

    let folder = first_output.as_ref().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let path = first_output.as_ref();

    send_with_actions(
        enabled,
        &summary,
        &body_lines.join("\n"),
        folder.as_ref(),
        path,
    );
}

fn short_error(e: &str) -> String {
    let first = e.lines().next().unwrap_or(e);
    let trimmed: String = first.chars().take(180).collect();
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts() {
        let mut r = BatchResult::default();
        r.outcomes.push(crate::convert::JobOutcome {
            input: "/a/x.png".into(),
            outputs: vec!["/a/x.webp".into()],
            error: None,
        });
        r.outcomes.push(crate::convert::JobOutcome {
            input: "/a/y.png".into(),
            outputs: vec![],
            error: Some("boom".into()),
        });
        assert_eq!(r.ok_count(), 1);
        assert_eq!(r.failed_count(), 1);
        assert_eq!(short_error("line1\nline2"), "line1");
    }
}
