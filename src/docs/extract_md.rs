use regex::Regex;
use serde_json::Value;

pub fn extract_md(html: &str) -> String {
    // Remove head section before processing
    let re = regex::Regex::new(r"<head>.*?</head>").unwrap();
    let html = re.replace(html, "");
    let re = regex::Regex::new(r"<script[^>]*>.*?</script>").unwrap();
    let html = re.replace(&html, "");
    let md = html2md::parse_html(&html);
    let md = extract_lines_after_package(&md);
    remove_backslashes(&remove_tags(&remove_markdown_links(&md)))
}

fn remove_markdown_links(input: &str) -> String {
    let re = regex::Regex::new(r"\[([^\[\]]+)\]\(([^)]+)\)").unwrap();
    let replaced = re.replace_all(input, |caps: &regex::Captures| {
        caps.get(1).unwrap().as_str().to_string()
    });
    replaced.to_string()
}

fn remove_backslashes(input: &str) -> String {
    input
        .lines() // Iterate over each line
        .map(|line| {
            if line.starts_with("//") || line.starts_with("///") {
                line.to_string() // Keep the line unchanged
            } else {
                line.replace("\\", "") // Remove all backslashes
            }
        })
        .collect::<Vec<_>>() // Collect the processed lines into a Vec
        .join("\n") // Recombine them into a single string
}

fn remove_tags(input: &str) -> String {
    // Regex to match <details ...> with any attributes
    let details_open_tag = Regex::new(r"<details[^>]*>").unwrap();
    let summary_open_tag = Regex::new(r"<summary[^>]*>").unwrap();
    let href_open_tag = Regex::new(r"<a[^>]*>").unwrap();

    // Regex to match </details>, <summary>, and </summary>
    let other_tags = Regex::new(r"</?details>|</?summary>|</?a>").unwrap();

    // Remove <details ...> tags
    let without_details_open = details_open_tag.replace_all(input, "");
    // Remove <summary ...> tags
    let without_summary_open = summary_open_tag.replace_all(&without_details_open, "");
    // Remove <a ...> tags
    let without_href_open = href_open_tag.replace_all(&without_summary_open, "");
    // Remove </details>, <summary>, and </summary> tags
    let result = other_tags.replace_all(&without_href_open, "");

    result.to_string()
}

fn extract_lines_after_package(input: &str) -> String {
    let mut lines = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .peekable();
    let mut name = String::new();
    let mut version = String::new();
    let mut line_cache = Vec::new();

    // Find the first line with `Docs.rs` and parse the next line
    while let Some(line) = lines.next() {
        line_cache.push(line);
        if line.contains("Docs.rs") {
            if let Some(next_line) = lines.next() {
                if let Ok(json) = serde_json::from_str::<Value>(next_line) {
                    if let (Some(n), Some(v)) = (json.get("name"), json.get("version")) {
                        name = n.as_str().map(|s| s.to_string()).unwrap_or_default();
                        version = v.as_str().map(|s| s.to_string()).unwrap_or_default();
                    }
                }
            }
            break;
        }

        // Fallback if we don't find name / version
        if line.contains(r#"<iframe src="/-/storage-change-detection.html" width="0" height="0" style="display: none">"#) {
            return lines.collect::<Vec<_>>().join("\n")
        }
    }

    if !name.is_empty() && !version.is_empty() {
        for line in lines.by_ref() {
            line_cache.push(line);
            if line.contains(&format!("[{name}](")) && line.contains(&format!(" {version}")) {
                break;
            }
        }
    }

    // Collect all remaining lines and return as a single string
    let resulting_lines = lines.collect::<Vec<_>>(); //.join("\n");
    if resulting_lines.len() <= 1 {
        return line_cache.join("\n");
    }
    resulting_lines.join("\n")
}
