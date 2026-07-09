// src-tauri/src/script.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptCounts {
    pub hiragana: usize,
    pub katakana: usize,
    pub kanji: usize,
}

pub fn count_script_chars(text: &str) -> ScriptCounts {
    let mut hiragana = 0;
    let mut katakana = 0;
    let mut kanji = 0;

    for c in text.chars() {
        match c {
            '\u{3040}'..='\u{309F}' => hiragana += 1,
            '\u{30A0}'..='\u{30FF}' => {
                if c != '\u{30FB}' {
                    katakana += 1;
                }
            }
            '\u{4E00}'..='\u{9FFF}' => kanji += 1,
            _ => {}
        }
    }

    ScriptCounts {
        hiragana,
        katakana,
        kanji,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptVerdict {
    Accept,
    Reject,
}

pub fn classify_script(text: &str) -> ScriptVerdict {
    if text.trim().is_empty() {
        return ScriptVerdict::Reject;
    }

    let counts = count_script_chars(text);
    let kana = counts.hiragana + counts.katakana;
    let total_cjk = counts.hiragana + counts.katakana + counts.kanji;

    if total_cjk == 0 {
        return ScriptVerdict::Reject;
    }

    // 1. If we have at least 2 kana, it's a solid CJK string (e.g. はい, アリス, こんにちは)
    if kana >= 2 {
        return ScriptVerdict::Accept;
    }

    // 2. If we have at least 1 Hiragana and at least 1 Kanji, accept (e.g. 食べる)
    if counts.hiragana >= 1 && counts.kanji >= 1 {
        return ScriptVerdict::Accept;
    }

    // 3. If we have at least 1 Katakana and at least 1 Kanji, accept (e.g. アニメ化)
    if counts.katakana >= 1 && counts.kanji >= 1 {
        return ScriptVerdict::Accept;
    }

    // 4. Kanji-only (no kana at all)
    if counts.kanji > 0 && kana == 0 {
        // Check for Simplified Chinese denylist characters/radicals
        if has_simplified_chinese_only_chars(text) {
            return ScriptVerdict::Reject;
        }

        // Limit the length of pure Kanji to avoid Chinese sentences (max 3 characters)
        if counts.kanji >= 1 && counts.kanji <= 3 {
            return ScriptVerdict::Accept;
        }
    }

    ScriptVerdict::Reject
}

fn has_simplified_chinese_only_chars(text: &str) -> bool {
    for c in text.chars() {
        match c {
            // Simplified radicals & common simplified-only characters
            '们' | '这' | '说' | '对' | '时' | '发' | '为' | '无' | '从' | '极' | '样' |
            '线' | '经' | '关' | '观' | '开' | '问' | '题' | '业' | '东' | '乐' | '气' |
            '\u{7E9F}' | // 纟
            '\u{95E8}' | // 门
            '\u{9963}' | // 饣
            '\u{9485}' | // 钅
            '\u{8BA0}'..='\u{8BAA}' // 讠 radical block
            => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_script_chars() {
        let counts = count_script_chars("あいうえお アイウエオ 漢字");
        assert_eq!(counts.hiragana, 5);
        assert_eq!(counts.katakana, 5);
        assert_eq!(counts.kanji, 2);
    }

    #[test]
    fn test_classify_script_verdicts() {
        // Acceptances
        assert_eq!(classify_script("出口"), ScriptVerdict::Accept);
        assert_eq!(classify_script("設定"), ScriptVerdict::Accept);
        assert_eq!(classify_script("駅"), ScriptVerdict::Accept);
        assert_eq!(classify_script("日本語の"), ScriptVerdict::Accept);

        // Rejections (Chinese blocks or stray/empty)
        assert_eq!(classify_script("你好世界"), ScriptVerdict::Reject);
        assert_eq!(classify_script("们"), ScriptVerdict::Reject);
        assert_eq!(classify_script("这"), ScriptVerdict::Reject);
        assert_eq!(classify_script("说"), ScriptVerdict::Reject);
        assert_eq!(classify_script("English only"), ScriptVerdict::Reject);
    }
}
