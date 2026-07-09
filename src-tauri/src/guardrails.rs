// src-tauri/src/guardrails.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub accepted: bool,
    pub reason: Option<String>,
    pub cleaned_text: String,
}

pub fn validate_translation(source: &str, candidate: &str) -> ValidationOutcome {
    // 1. Strip <think> tags and reasoning text
    let mut text = candidate.to_string();
    if let Some(start_idx) = text.find("<think>")
        && let Some(end_idx) = text.find("</think>")
        && end_idx > start_idx
    {
        text.drain(start_idx..end_idx + "</think>".len());
    }
    let mut text = text.trim().to_string();

    // Strip <text> and </text> tags
    if text.contains("<text>") || text.contains("</text>") {
        text = text.replace("<text>", "").replace("</text>", "");
    }
    let mut text = text.trim().to_string();

    // 2. Strip conversational preambles/meta-commentary
    let preambles = [
        "sure, here's the translation:",
        "sure! here is the english translation:",
        "sure, here is the translation:",
        "sure, here is the english translation:",
        "here is the translation:",
        "here is the english translation:",
        "translation:",
        "english translation:",
    ];
    let text_lower = text.to_ascii_lowercase();
    for preamble in &preambles {
        if text_lower.starts_with(preamble) {
            text = text[preamble.len()..].trim().to_string();
            break;
        }
    }

    // Strip wrapping quotes
    if ((text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\'')))
        && text.len() >= 2
    {
        text = text[1..text.len() - 1].trim().to_string();
    }

    // 3. Reject empty translations
    if text.is_empty() {
        return ValidationOutcome {
            accepted: false,
            reason: Some("Empty translation".to_string()),
            cleaned_text: text,
        };
    }

    // 4. Reject exact echoes
    if text.eq_ignore_ascii_case(source) {
        return ValidationOutcome {
            accepted: false,
            reason: Some("Exact source echo".to_string()),
            cleaned_text: text,
        };
    }

    // 5. Reject residual CJK/Japanese text
    let counts = crate::script::count_script_chars(&text);
    if counts.hiragana > 0 || counts.katakana > 0 || counts.kanji > 0 {
        return ValidationOutcome {
            accepted: false,
            reason: Some("Contains residual CJK/Japanese characters".to_string()),
            cleaned_text: text,
        };
    }

    // 6. Reject model refusal phrases
    let lower_text = text.to_ascii_lowercase();
    if lower_text.contains("cannot translate")
        || lower_text.contains("unable to translate")
        || lower_text.contains("as an ai")
        || lower_text.contains("not appropriate")
    {
        return ValidationOutcome {
            accepted: false,
            reason: Some("Refusal phrase detected".to_string()),
            cleaned_text: text,
        };
    }

    // 7. Reject length anomalies (repetition / hallucination loops)
    let source_len = source.chars().count();
    let candidate_len = text.chars().count();
    if candidate_len > 30 && candidate_len > 6 * source_len {
        return ValidationOutcome {
            accepted: false,
            reason: Some("Translation length anomaly".to_string()),
            cleaned_text: text,
        };
    }

    ValidationOutcome {
        accepted: true,
        reason: None,
        cleaned_text: text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_think_tag_stripping() {
        let output = validate_translation(
            "終了",
            "<think>\nThinking about translating exit.\n</think>\nExit",
        );
        assert!(output.accepted);
        assert_eq!(output.cleaned_text, "Exit");
    }

    #[test]
    fn test_preamble_stripping() {
        let output =
            validate_translation("設定", "Sure! Here is the English translation:\nSettings");
        assert!(output.accepted);
        assert_eq!(output.cleaned_text, "Settings");
    }

    #[test]
    fn test_echo_rejection() {
        let output = validate_translation("設定", "設定");
        assert!(!output.accepted);
    }

    #[test]
    fn test_residual_cjk_rejection() {
        let output = validate_translation("閉じる", "Close (閉じる)");
        assert!(!output.accepted);
    }

    #[test]
    fn test_refusal_rejection() {
        let output = validate_translation("秘密", "I cannot translate this text.");
        assert!(!output.accepted);
    }

    #[test]
    fn test_length_anomaly_rejection() {
        let output = validate_translation(
            "はい",
            "Yes, indeed! This is a very long explanation that is absolutely not a direct translation of the source word.",
        );
        assert!(!output.accepted);
    }

    #[test]
    fn test_text_tag_stripping() {
        let output = validate_translation("閉じる", "<text>Close</text>");
        assert!(output.accepted);
        assert_eq!(output.cleaned_text, "Close");
    }
}
