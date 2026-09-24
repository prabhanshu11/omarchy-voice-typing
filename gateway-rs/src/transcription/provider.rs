//! Which speech-to-text backend a new recording uses.
//!
//! Read fresh at the start of every recording, so switching needs no restart:
//!   1. file `~/.config/voice-typing/stt-provider` (first word: `local` | `deepgram`)
//!   2. env `STT_PROVIDER`
//!   3. default `deepgram`
//!
//! `local` = no Deepgram at all: audio is buffered and sent at commit to the
//! fine-tuned Whisper on the desktop GPU (LAN_WHISPER_URL), falling back to
//! the laptop's local-whisper (LOCAL_WHISPER_URL). Switch with the
//! `voice-stt-provider` script in local-bootstrapping.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Deepgram,
    Local,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Deepgram => "deepgram",
            Provider::Local => "local",
        }
    }
}

pub fn provider_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/voice-typing/stt-provider")
}

fn parse(s: &str) -> Option<Provider> {
    match s.split_whitespace().next()?.to_ascii_lowercase().as_str() {
        "local" | "whisper" | "local-whisper" => Some(Provider::Local),
        "deepgram" | "nova-2" => Some(Provider::Deepgram),
        _ => None,
    }
}

pub fn current() -> Provider {
    if let Ok(s) = std::fs::read_to_string(provider_file()) {
        if let Some(p) = parse(&s) {
            return p;
        }
    }
    std::env::var("STT_PROVIDER")
        .ok()
        .and_then(|s| parse(&s))
        .unwrap_or(Provider::Deepgram)
}

/// Words the user asked the model to concentrate on (web app list), joined
/// for faster-whisper's `hotwords`. One word/phrase per line, `#` comments.
pub fn hotwords() -> Option<String> {
    let path = std::env::var("STT_VOCAB_FILE").map(PathBuf::from).unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("Programs/voice-stt/labels/vocab.txt")
    });
    let text = std::fs::read_to_string(path).ok()?;
    let words: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names() {
        assert_eq!(parse("local\n"), Some(Provider::Local));
        assert_eq!(parse(" Deepgram "), Some(Provider::Deepgram));
        assert_eq!(parse("whisper"), Some(Provider::Local));
        assert_eq!(parse("nonsense"), None);
        assert_eq!(parse(""), None);
    }
}
