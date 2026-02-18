use std::path::Path;

use crate::error::GatewayError;

/// A single spelling replacement rule: multiple "from" variants → one "to" word.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReplacementConfig {
    pub from: Vec<String>,
    pub to: String,
}

/// Flattened replacement: one from → one to (used at runtime for fast lookup).
#[derive(Debug, Clone)]
pub struct CustomSpelling {
    pub from: String,
    pub to: String,
}

/// Load replacement rules from a JSON file.
///
/// The file format is an array of `{ "from": ["variant1", "variant2"], "to": "correct" }`.
/// Each entry is flattened into individual `CustomSpelling` pairs.
///
/// Returns an empty vec (not an error) if the file doesn't exist.
pub fn load_replacements(path: &Path) -> Result<Vec<CustomSpelling>, GatewayError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(GatewayError::Io(e)),
    };

    let configs: Vec<ReplacementConfig> = serde_json::from_str(&contents)?;

    let spellings = configs
        .into_iter()
        .flat_map(|config| {
            config.from.into_iter().map(move |from| CustomSpelling {
                from,
                to: config.to.clone(),
            })
        })
        .collect();

    Ok(spellings)
}

/// Apply custom spelling replacements to a transcript.
///
/// Performs case-insensitive whole-word replacement. This matches the Go behavior
/// where each "from" word in the transcript is replaced with its "to" equivalent.
pub fn apply_replacements(text: &str, spellings: &[CustomSpelling]) -> String {
    let mut result = text.to_string();
    for spelling in spellings {
        // Case-insensitive whole-word replacement
        let mut new_result = String::new();
        let mut remaining = result.as_str();
        let from_lower = spelling.from.to_lowercase();

        while !remaining.is_empty() {
            if let Some(pos) = remaining.to_lowercase().find(&from_lower) {
                // Check word boundaries
                let before_ok =
                    pos == 0 || !remaining.as_bytes()[pos - 1].is_ascii_alphanumeric();
                let end = pos + spelling.from.len();
                let after_ok =
                    end >= remaining.len() || !remaining.as_bytes()[end].is_ascii_alphanumeric();

                if before_ok && after_ok {
                    new_result.push_str(&remaining[..pos]);
                    new_result.push_str(&spelling.to);
                    remaining = &remaining[end..];
                } else {
                    new_result.push_str(&remaining[..end]);
                    remaining = &remaining[end..];
                }
            } else {
                new_result.push_str(remaining);
                break;
            }
        }

        result = new_result;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_replacements_basic() {
        let spellings = vec![CustomSpelling {
            from: "Dovac".to_string(),
            to: "Dvorak".to_string(),
        }];
        assert_eq!(apply_replacements("I use Dovac layout", &spellings), "I use Dvorak layout");
    }

    #[test]
    fn test_apply_replacements_case_insensitive() {
        let spellings = vec![CustomSpelling {
            from: "dovac".to_string(),
            to: "Dvorak".to_string(),
        }];
        assert_eq!(apply_replacements("I use DOVAC", &spellings), "I use Dvorak");
    }

    #[test]
    fn test_apply_replacements_word_boundary() {
        let spellings = vec![CustomSpelling {
            from: "or".to_string(),
            to: "OR".to_string(),
        }];
        // "or" inside "word" should NOT be replaced
        assert_eq!(apply_replacements("word or other", &spellings), "word OR other");
    }

    #[test]
    fn test_apply_replacements_empty() {
        assert_eq!(apply_replacements("hello world", &[]), "hello world");
    }
}
