//! Minimalne narzędzia do wyciągania tekstu z HTML (serwisy z tekstami bez API).

use once_cell::sync::Lazy;
use regex::Regex;

/// Zwraca wnętrza wszystkich elementów `<div>`, których znacznik otwierający zawiera `marker`
/// (np. `data-lyrics-container="true"`). Obsługuje zagnieżdżone `<div>`.
pub fn divs_with(html: &str, marker: &str) -> Vec<String> {
    let mut out = vec![];
    let mut from = 0;
    while let Some(pos) = html[from..].find(marker) {
        let at = from + pos;
        if html[..at].rfind("<div").is_none() {
            break;
        }
        let Some(gt) = html[at..].find('>') else { break };
        let start = at + gt + 1;
        match matching_close(html, start) {
            Some(end) => {
                out.push(html[start..end].to_string());
                from = end;
            }
            None => break,
        }
    }
    out
}

/// Pozycja `</div>` zamykającego element, którego treść zaczyna się w `start`.
/// Działa na bajtach – wycinki `&str` w środku znaku wielobajtowego (np. „ą”) kończyły się paniką.
fn matching_close(html: &str, start: usize) -> Option<usize> {
    let b = html.as_bytes();
    let mut depth = 1usize;
    let mut i = start;
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &b[i..];
            if rest.len() >= 5 && rest[..4].eq_ignore_ascii_case(b"<div") && matches!(rest[4], b' ' | b'>' | b'\n' | b'\t' | b'\r') {
                depth += 1;
            } else if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case(b"</div>") {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Usuwa elementy `<div>` z danym markerem razem z zawartością.
pub fn remove_divs_with(html: &str, marker: &str) -> String {
    let mut s = html.to_string();
    while let Some(at) = s.find(marker) {
        let Some(open) = s[..at].rfind("<div") else { break };
        let Some(gt) = s[at..].find('>') else { break };
        let start = at + gt + 1;
        let Some(end) = matching_close(&s, start) else { break };
        s.replace_range(open..end + 6, "");
    }
    s
}

/// Zamienia HTML na zwykły tekst: `<br>` i `</p>` → nowa linia, reszta znaczników usunięta.
pub fn to_text(html: &str) -> String {
    static BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>|</p>|</div>").unwrap());
    static TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<[^>]*>").unwrap());
    let s = BR.replace_all(html, "\n");
    let s = TAG.replace_all(&s, "");
    let s = decode_entities(&s);
    tidy_lines(&s)
}

/// Przycina linie, usuwa nadmiarowe puste linie (najwyżej jedna pod rząd).
pub fn tidy_lines(s: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for line in s.lines().map(|l| l.trim()) {
        if line.is_empty() && out.last().map(|l| l.is_empty()).unwrap_or(true) {
            continue;
        }
        out.push(line);
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

pub fn decode_entities(s: &str) -> String {
    static ENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);").unwrap());
    ENT.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let ch = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = e.strip_prefix('#') {
            d.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match e {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "rsquo" | "lsquo" => Some('\''),
                "rdquo" | "ldquo" | "bdquo" => Some('"'),
                _ => None,
            }
        };
        ch.map(|c| c.to_string()).unwrap_or_else(|| c[0].to_string())
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_divs_and_text() {
        let h = r#"<div class="a"><div data-lyrics-container="true" class="x"><div data-exclude-from-selection="true"><div>12 Contributors</div></div>Raz<br/>Dwa &amp; trzy<br><a href="/x"><span>Cztery</span></a><br>Pięć</div><div data-lyrics-container="true">Sześć</div></div>"#;
        let parts = divs_with(h, "data-lyrics-container=\"true\"");
        assert_eq!(parts.len(), 2);
        let first = to_text(&remove_divs_with(&parts[0], "data-exclude-from-selection"));
        assert_eq!(first, "Raz\nDwa & trzy\nCztery\nPięć");
        assert_eq!(to_text(&parts[1]), "Sześć");
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let h = "<div data-x=\"1\"><ąą<ść</div>żółć<div data-x=\"1\">źdźbło<ę";
        let parts = divs_with(h, "data-x=\"1\"");
        assert_eq!(parts[0], "<ąą<ść");
        assert_eq!(remove_divs_with(h, "data-x"), "żółć<div data-x=\"1\">źdźbło<ę");
    }
}
