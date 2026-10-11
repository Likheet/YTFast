//! Lyrics in another language, and in Latin letters (Japanese in rōmaji,
//! Korean, Russian, Hindi...). People's translations and rōmaji come from
//! Musixmatch (`musixmatch.rs`) when it has them; the rest from Google
//! Translate's free web address, the one browser add-ons use (Better
//! Lyrics among them). It is unofficial, so it may change or stop: then
//! the lyrics show untranslated. It answers apps' HTTP/2 requests with
//! "too many requests" (seen October 2026), so it is asked over HTTP/1.1
//! ([`client`]). One request for a whole song (two for a very long one).

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::redact;

const GOOGLE: &str = "https://translate.googleapis.com/translate_a/single";
/// Put between lines, so they come back apart: Google keeps it, while it
/// runs Japanese lines together in Latin letters.
const MARK: char = '¶';
/// The most characters of lyrics in one request.
const MOST_PER_REQUEST: usize = 4000;
/// How long a request may take.
const PATIENCE: Duration = Duration::from_secs(15);
/// How many of the lines (of 10) people's translation must have for it
/// to be these lyrics'.
const PEOPLES_ENOUGH: usize = 8;

/// What each line of lyrics becomes, `None` where there is nothing to add.
pub type Lines = Vec<Option<String>>;

/// Lyrics translated: each line in the language asked for, and in Latin
/// letters, `None` where there is nothing to add (a line already in that
/// language or those letters, or one that did not come back).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Translated {
    /// The lyrics' language as Google read it (ISO 639-1), when it was
    /// asked.
    pub language: Option<String>,
    pub lines: Lines,
    pub latin: Lines,
    /// Who translated them: "Musixmatch" (people) or "Google Translate".
    pub source: String,
}

impl Translated {
    /// Nothing to show: every line is already as wanted.
    pub fn is_empty(&self) -> bool {
        self.lines.iter().chain(&self.latin).all(Option::is_none)
    }
}

/// The client for Google Translate: HTTP/1.1 only, which it answers.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .http1_only()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("the HTTP client settings are valid")
}

/// People's translation of a song's lyrics, and their rōmaji: each line
/// as written, and as translated.
#[derive(Clone, Copy, Debug, Default)]
pub struct Peoples<'a> {
    pub translation: Option<&'a [(String, String)]>,
    pub latin: Option<&'a [(String, String)]>,
}

/// `lines` in `language` (ISO 639-1), and in Latin letters when they are
/// written in another script: people's (`peoples`) where it fits these
/// lines, else Google Translate's (asked with `http`, from [`client`]).
/// Google is asked only for what people's leave out.
pub async fn translate(
    http: &reqwest::Client,
    lines: &[String],
    language: &str,
    peoples: Peoples<'_>,
) -> Result<Translated, String> {
    let translation = peoples.translation.and_then(|pairs| lay_over(lines, pairs));
    let latin = peoples.latin.and_then(|pairs| lay_over(lines, pairs));
    if let Some(translated) = &translation
        && (latin.is_some() || !needs_latin(lines))
    {
        return Ok(Translated {
            language: None,
            lines: translated.clone(),
            latin: latin.unwrap_or_else(|| vec![None; lines.len()]),
            source: "Musixmatch".into(),
        });
    }
    let mut google = google(http, lines, language).await?;
    if let Some(translated) = translation {
        google.lines = translated;
        google.source = "Musixmatch".into();
    }
    if let Some(latin) = latin {
        google.latin = latin;
    }
    Ok(google)
}

/// People's translation laid over the lines, by their words (letters and
/// numbers, whatever the capitals, quotes and spaces). `None` when it has
/// too few of them to be these lyrics'.
fn lay_over(lines: &[String], pairs: &[(String, String)]) -> Option<Lines> {
    let key = |line: &str| -> String {
        line.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let translations: HashMap<String, &str> = pairs
        .iter()
        .filter(|(_, to)| !to.trim().is_empty())
        .map(|(from, to)| (key(from), to.trim()))
        .collect();
    let (mut wanted, mut found) = (0, 0);
    let laid: Lines = lines
        .iter()
        .map(|line| {
            let line = key(line);
            if line.is_empty() {
                return None;
            }
            wanted += 1;
            let translation = translations.get(&line).map(|t| t.to_string());
            found += usize::from(translation.is_some());
            translation
        })
        .collect();
    (wanted > 0 && found * 10 >= wanted * PEOPLES_ENOUGH).then_some(laid)
}

/// Whether any line is written in letters other than Latin ones.
fn needs_latin(lines: &[String]) -> bool {
    lines.iter().flat_map(|line| line.chars()).any(|c| {
        let code = c as u32;
        c.is_alphabetic()
            && !(code < 0x0250
                || (0x1E00..=0x1EFF).contains(&code)
                || (0xFF21..=0xFF5A).contains(&code))
    })
}

/// Google Translate's translation of `lines` into `language`, and the
/// lines in Latin letters, in as few requests as the lyrics' length
/// allows.
async fn google(
    http: &reqwest::Client,
    lines: &[String],
    language: &str,
) -> Result<Translated, String> {
    let mut translated = Translated {
        source: "Google Translate".into(),
        ..Translated::default()
    };
    for part in parts(lines) {
        let text = part
            .iter()
            .map(|line| line.replace(MARK, " "))
            .collect::<Vec<_>>()
            .join(&format!("\n{MARK}\n"));
        let reply = ask_google(http, &text, language).await?;
        let (detected, lines, latin) = read_reply(&reply, part.len())
            .ok_or_else(|| "Google Translate's answer could not be read".to_string())?;
        translated.language = translated.language.or(detected);
        translated.lines.extend(lines);
        translated.latin.extend(latin);
    }
    // Lines already as wanted add nothing.
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let in_language = translated.language.as_deref() == Some(language);
    for (i, line) in lines.iter().enumerate() {
        if in_language
            || translated.lines[i]
                .as_deref()
                .is_some_and(|t| same(t, line))
        {
            translated.lines[i] = None;
        }
        if translated.latin[i]
            .as_deref()
            .is_some_and(|t| same(t, line))
        {
            translated.latin[i] = None;
        }
    }
    Ok(translated)
}

/// The lines in groups of at most [`MOST_PER_REQUEST`] characters.
fn parts(lines: &[String]) -> Vec<&[String]> {
    let mut parts = Vec::new();
    let (mut from, mut size) = (0, 0);
    for (i, line) in lines.iter().enumerate() {
        let length = line.chars().count() + 3;
        if size + length > MOST_PER_REQUEST && i > from {
            parts.push(&lines[from..i]);
            (from, size) = (i, 0);
        }
        size += length;
    }
    if from < lines.len() {
        parts.push(&lines[from..]);
    }
    parts
}

async fn ask_google(http: &reqwest::Client, text: &str, language: &str) -> Result<Value, String> {
    let failed = |e: reqwest::Error| {
        format!(
            "could not reach Google Translate: {}",
            redact::urls(&e.to_string())
        )
    };
    let request = http
        .post(GOOGLE)
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", language),
            ("dt", "t"),
            ("dt", "rm"),
        ])
        .header(reqwest::header::USER_AGENT, crate::lyrics::USER_AGENT)
        .form(&[("q", text)])
        .send();
    let response = tokio::time::timeout(PATIENCE, request)
        .await
        .map_err(|_| "Google Translate did not answer in time".to_string())?
        .map_err(failed)?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Google Translate answered with HTTP {}",
            status.as_u16()
        ));
    }
    let text = response.text().await.map_err(failed)?;
    serde_json::from_str(&text).map_err(|e| format!("Google Translate's answer: {e}"))
}

/// Google's answer for `count` lines: the language it read, each line
/// translated, and each in Latin letters. Its first part is pieces of the
/// translation (each `[translated, original, ...]`), then, when the lines
/// are in another script, one `[null, null, ..., in Latin letters]`; the
/// third is the language read. `None` when it is not that shape.
fn read_reply(reply: &Value, count: usize) -> Option<(Option<String>, Lines, Lines)> {
    let pieces = reply.get(0)?.as_array()?;
    let mut translated = String::new();
    let mut latin = None;
    for piece in pieces {
        match (
            piece.get(0).and_then(Value::as_str),
            piece.get(3).and_then(Value::as_str),
        ) {
            (Some(words), _) => translated.push_str(words),
            (None, Some(letters)) => latin = Some(letters),
            _ => {}
        }
    }
    let language = reply.get(2).and_then(Value::as_str).map(str::to_string);
    // Back into lines; when they do not come back as many, none are used.
    let apart = |text: &str| -> Option<Lines> {
        let lines: Lines = text
            .split(MARK)
            .map(|line| Some(line.trim().to_string()).filter(|line| !line.is_empty()))
            .collect();
        (lines.len() == count).then_some(lines)
    };
    Some((
        language,
        apart(&translated).unwrap_or_else(|| vec![None; count]),
        latin.and_then(apart).unwrap_or_else(|| vec![None; count]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    /// Google's answer in its own shape (made-up lines, as Google sent
    /// them back for Japanese into English).
    #[test]
    fn googles_answer_line_by_line() {
        let reply = json!([
            [
                [
                    "It's nice weather today.\n¶\n",
                    "今日はいい天気ですね\n¶\n",
                    null,
                    null,
                    1
                ],
                [
                    "I like music\n¶\n\n¶\n",
                    "私は音楽が好きです\n¶\n\n¶\n",
                    null,
                    null,
                    1
                ],
                [
                    "stars shine in the night sky",
                    "夜空に星が光る",
                    null,
                    null,
                    1
                ],
                [
                    null,
                    null,
                    null,
                    "Kyō wa ī tenkidesu ne¶ watashi wa ongaku ga sukidesu¶ ¶ yozora ni hoshi ga hikaru"
                ]
            ],
            null,
            "ja"
        ]);
        let (language, lines, latin) = read_reply(&reply, 4).unwrap();
        assert_eq!(language.as_deref(), Some("ja"));
        assert_eq!(
            lines,
            [
                Some("It's nice weather today.".into()),
                Some("I like music".into()),
                None,
                Some("stars shine in the night sky".into())
            ]
        );
        assert_eq!(latin[0].as_deref(), Some("Kyō wa ī tenkidesu ne"));
        assert_eq!(latin[2], None);
        assert_eq!(latin[3].as_deref(), Some("yozora ni hoshi ga hikaru"));
        // Lines that do not come back as many: none of them.
        let (_, lines, latin) = read_reply(&reply, 5).unwrap();
        assert!(lines.iter().chain(&latin).all(Option::is_none));
        // In Latin letters already: no such part.
        let spanish = json!([
            [[
                "Today the weather is good",
                "Hoy hace buen tiempo",
                null,
                null,
                1
            ]],
            null,
            "es"
        ]);
        let (language, lines, latin) = read_reply(&spanish, 1).unwrap();
        assert_eq!(language.as_deref(), Some("es"));
        assert_eq!(lines[0].as_deref(), Some("Today the weather is good"));
        assert_eq!(latin, [None]);
        assert_eq!(read_reply(&json!({"error": 1}), 1), None);
    }

    /// People's translation and rōmaji that fit the lines are all that is
    /// shown, and Google is not asked (its client here reaches nothing).
    #[tokio::test]
    async fn peoples_translation_and_romaji_need_no_google() {
        let nowhere = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let lines = strings(&["夜空に星が光る", "", "君の声"]);
        let pairs = |a: &str, b: &str| {
            vec![
                (lines[0].clone(), a.to_string()),
                (lines[2].clone(), b.to_string()),
            ]
        };
        let english = pairs("Stars shine in the night sky", "Your voice");
        let romaji = pairs("Yozora ni hoshi ga hikaru", "Kimi no koe");
        let peoples = Peoples {
            translation: Some(&english),
            latin: Some(&romaji),
        };
        let translated = translate(&nowhere, &lines, "en", peoples).await.unwrap();
        assert_eq!(translated.source, "Musixmatch");
        assert_eq!(translated.lines[2].as_deref(), Some("Your voice"));
        assert_eq!(
            translated.latin[0].as_deref(),
            Some("Yozora ni hoshi ga hikaru")
        );
        assert_eq!(translated.latin[1], None);
        // Without people's rōmaji, Japanese needs Google for its letters.
        let only_english = Peoples {
            translation: Some(&english),
            latin: None,
        };
        assert!(
            translate(&nowhere, &lines, "en", only_english)
                .await
                .is_err()
        );
    }

    #[test]
    fn peoples_translation_where_it_fits() {
        let lines = strings(&["I’m fine,", "", "Second LINE", "Third line"]);
        let pairs = [
            ("I'm fine".to_string(), "Je vais bien".to_string()),
            ("second line".to_string(), "Deuxième".to_string()),
            ("Third line!".to_string(), "Troisième".to_string()),
        ];
        assert_eq!(
            lay_over(&lines, &pairs),
            Some(vec![
                Some("Je vais bien".into()),
                None,
                Some("Deuxième".into()),
                Some("Troisième".into())
            ])
        );
        // Too few of the lines: another song's lyrics.
        assert_eq!(lay_over(&lines, &pairs[..1]), None);
        assert_eq!(lay_over(&strings(&["", "♪"]), &pairs), None);
    }

    #[test]
    fn which_lines_need_latin_letters_and_how_they_are_sent() {
        assert!(!needs_latin(&strings(&[
            "Hola, ¿qué tal?",
            "Ça va très bien",
            "Tiếng Việt",
            "♪ 123"
        ])));
        for other in ["夜空に", "사랑해", "Привет", "नमस्ते", "Γειά"] {
            assert!(needs_latin(&strings(&["Hello", other])), "{other}");
        }
        // Long lyrics go in parts, each line whole.
        let long: Vec<String> = (0..300).map(|i| format!("{i:>30}")).collect();
        let parts = parts(&long);
        assert!(parts.len() >= 2);
        assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), 300);
        assert!(
            parts
                .iter()
                .all(|p| p.iter().map(|l| l.len() + 3).sum::<usize>() <= MOST_PER_REQUEST)
        );
        assert_eq!(super::parts(&[]).len(), 0);
    }

    /// Asks Google Translate itself when `YTFAST_TEST_TRANSLATE` is set;
    /// otherwise does nothing. Made-up Japanese lines come back in English
    /// and in Latin letters, line by line; English needs nothing.
    #[tokio::test]
    async fn google_translate_itself() {
        if std::env::var("YTFAST_TEST_TRANSLATE").is_err() {
            return;
        }
        let http = client();
        let lines = strings(&["今日はいい天気ですね", "", "夜空に星が光る"]);
        let translated = translate(&http, &lines, "en", Peoples::default())
            .await
            .unwrap();
        eprintln!("{translated:?}");
        assert_eq!(translated.language.as_deref(), Some("ja"));
        assert!(
            translated.lines[0]
                .as_deref()
                .is_some_and(|l| l.to_lowercase().contains("weather"))
        );
        assert_eq!((&translated.lines[1], &translated.latin[1]), (&None, &None));
        assert!(
            translated.latin[2]
                .as_deref()
                .is_some_and(|l| l.to_lowercase().contains("hoshi"))
        );
        let english = translate(
            &http,
            &strings(&["The weather is nice today"]),
            "en",
            Peoples::default(),
        )
        .await
        .unwrap();
        assert!(english.is_empty(), "{english:?}");
    }
}
