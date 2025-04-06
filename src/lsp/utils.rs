use lsp_types::{GotoDefinitionResponse, MarkedString};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use url::Url;

pub fn get_location_contents(
    response: GotoDefinitionResponse,
) -> Result<Vec<(String, PathBuf)>, std::io::Error> {
    let urls = match response {
        GotoDefinitionResponse::Scalar(location) => vec![location.uri],
        GotoDefinitionResponse::Array(locations) => {
            locations.into_iter().map(|loc| loc.uri).collect()
        }
        GotoDefinitionResponse::Link(links) => {
            links.into_iter().map(|link| link.target_uri).collect()
        }
    };

    let mut known_files = HashSet::new();

    let mut contents = Vec::new();
    for url in urls {
        if known_files.contains(&url) {
            continue;
        }
        known_files.insert(url.clone());
        // Convert the URL to a file path
        let path = url_to_file_path(&url)?;
        // Read the file contents
        let content = fs::read_to_string(&path)?;
        contents.push((content, path));
    }

    Ok(contents)
}

pub fn format_marked_string(marked_string: &MarkedString) -> String {
    match marked_string {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(language_string) => format!(
            "```{}```\n{}",
            language_string.language, language_string.value
        ),
    }
}

// Helper function to convert a URL to a file path
fn url_to_file_path(url: &Url) -> Result<PathBuf, std::io::Error> {
    url.to_file_path().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Invalid file URL: {}", url),
        )
    })
}
