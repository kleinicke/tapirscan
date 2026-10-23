//! Structured interpretation of decoded text: GS1 element strings, standard
//! QR payloads (URL, Wi-Fi, contacts, events, messages, locations), payment
//! slips (Swiss QR-bill, EPC/SEPA), HIBC, VINs, AAMVA identity documents and
//! ISBNs.
//!
//! Parsing is separate from scanning: it never changes what the scanner reads,
//! and text that matches no known structure has no content.

use serde::Serialize;

pub use crate::content_kind::ContentKind;

/// One named value. Names repeat where the payload repeats them (several
/// phone numbers of a contact, several GS1 elements with one AI).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Field {
    /// Field name: an application identifier such as `"01"` for GS1, otherwise
    /// a documented camelCase name such as `"ssid"`.
    pub name: String,
    /// Field value, unescaped but otherwise as encoded (dates stay unconverted).
    pub value: String,
}

/// Structured content of a decoded payload, with fields in payload order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Content {
    /// Recognized payload kind.
    pub kind: ContentKind,
    /// Named values in payload order; absent and empty values are omitted.
    pub fields: Vec<Field>,
}

impl Content {
    /// First value of a field.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.value.as_str())
    }

    /// Every value of a field, in payload order.
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.fields
            .iter()
            .filter(move |field| field.name == name)
            .map(|field| field.value.as_str())
    }

    fn new(kind: ContentKind) -> Self {
        Self {
            kind,
            fields: Vec::new(),
        }
    }

    fn push(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        if !value.is_empty() {
            self.fields.push(Field {
                name: name.into(),
                value,
            });
        }
    }

    fn nonempty(self) -> Option<Self> {
        (!self.fields.is_empty()).then_some(self)
    }
}

/// Recognize the structure of decoded text.
///
/// `gs1` is the reader's GS1 flag (`Barcode::gs1`): when true the text is
/// parsed as a GS1 element string with `<GS>` (0x1D) separators. Text that
/// starts with a symbology identifier (`]C1`, `]d2`, `]Q3`, `]e0`, `]J1`) or
/// uses the bracketed human-readable form `(01)…` is recognized without it.
/// Returns `None` when the text matches no supported structure or fails its
/// checks (check digits, mandatory lines).
#[must_use]
pub fn parse_content(text: &str, gs1: bool) -> Option<Content> {
    // A GS1 QR Code or Data Matrix may hold a Digital Link URL instead of
    // element strings, so a failed element parse falls through.
    if gs1 && let Some(content) = gs1_elements(text) {
        return Some(content);
    }
    for prefix in ["]C1", "]d2", "]Q3", "]e0", "]J1"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return gs1_elements(rest);
        }
    }
    if text.starts_with('(')
        && let Some(content) = gs1_bracketed(text)
    {
        return Some(content);
    }
    let upper: String = text
        .chars()
        .take(12)
        .collect::<String>()
        .to_ascii_uppercase();
    let starts = |prefix: &str| upper.starts_with(prefix);
    if starts("HTTP://") || starts("HTTPS://") {
        return web_url(text);
    }
    if starts("WWW.")
        && let Some(content) = web_url(text)
    {
        return Some(content);
    }
    if starts("URLTO:") {
        return urlto(&text[6..]);
    }
    if starts("MEBKM:") {
        return mebkm(&text[6..]);
    }
    if starts("MAILTO:") {
        return mailto(&text[7..]);
    }
    if starts("MATMSG:") {
        return matmsg(&text[7..]);
    }
    if starts("SMTP:") {
        return smtp(&text[5..]);
    }
    if starts("TEL:") {
        return Some(single(ContentKind::Phone, "number", text[4..].trim()));
    }
    if starts("SMSTO:") || starts("MMSTO:") {
        return smsto(&text[6..]);
    }
    if starts("SMS:") || starts("MMS:") {
        return sms_uri(&text[4..]);
    }
    if starts("GEO:") {
        return geo(&text[4..]);
    }
    if starts("WIFI:") {
        return wifi(&text[5..]);
    }
    if starts("MECARD:") {
        return mecard(&text[7..]);
    }
    if starts("BIZCARD:") {
        return bizcard(&text[8..]);
    }
    if starts("BEGIN:VCARD") {
        return vcard(text);
    }
    if starts("BEGIN:VEVENT") || starts("BEGIN:VCALEN") {
        return vevent(text);
    }
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    match lines.first().copied() {
        Some("SPC") => return swiss_qr_bill(&lines),
        Some("BCD") => return epc_payment(&lines),
        _ => {}
    }
    if text.starts_with('@') && (text.contains("ANSI ") || text.contains("AAMVA")) {
        return aamva(text);
    }
    if text.starts_with('+') {
        return hibc(text);
    }
    isbn(text)
        .or_else(|| email_address(text))
        .or_else(|| vin(text))
}

fn single(kind: ContentKind, name: &str, value: &str) -> Content {
    let mut content = Content::new(kind);
    content.push(name, value);
    content
}

// ---------------------------------------------------------------- GS1

/// Length of the application identifier at the start of `s`, from its first
/// two digits (GS1 General Specifications, section 3).
fn ai_length(s: &[u8]) -> Option<usize> {
    if s.len() < 2 || !s[..2].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let length = match (s[0] - b'0') * 10 + (s[1] - b'0') {
        0..=3 | 10..=22 | 30 | 37 | 90..=99 => 2,
        23..=25 | 40..=42 | 71 => 3,
        31..=36 | 39 | 43 | 70 | 72 | 80..=82 => 4,
        _ => return None,
    };
    (s.len() >= length && s[..length].iter().all(u8::is_ascii_digit)).then_some(length)
}

/// Data length of AIs with a predefined length; `None` means variable length,
/// terminated by `<GS>` or the end of the string.
fn ai_fixed_length(ai: &[u8]) -> Option<usize> {
    match &ai[..2] {
        b"00" => Some(18),
        b"01" | b"02" | b"03" => Some(14),
        b"11" | b"12" | b"13" | b"15" | b"16" | b"17" | b"31" | b"32" | b"33" | b"34" | b"35"
        | b"36" => Some(6),
        b"20" => Some(2),
        b"41" => Some(13),
        _ => None,
    }
}

/// Nonempty digits with a valid GS1 mod-10 check digit (GTIN, SSCC, GLN).
fn gs1_check_digit(digits: &str) -> bool {
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits
            .bytes()
            .rev()
            .enumerate()
            .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
            .sum::<u32>()
            % 10
            == 0
}

/// Whether `value` is valid data for `ai` (GS1 General Specifications, section 3):
/// fixed-length numeric AIs need exactly their length, identification keys a valid
/// check digit, and every element a value.
fn gs1_element_valid(ai: &str, value: &str) -> bool {
    let digits = |range: std::ops::Range<usize>| {
        value
            .get(range)
            .is_some_and(|part| part.bytes().all(|b| b.is_ascii_digit()))
    };
    let keyed = |key: usize, max: usize| {
        (key..=max).contains(&value.len()) && value.get(..key).is_some_and(gs1_check_digit)
    };
    match ai {
        // SSCC, GTIN, GLN, GSIN and GSRN: the whole value is the key.
        "00" | "01" | "02" | "03" | "402" | "410" | "411" | "412" | "413" | "414" | "415"
        | "416" | "417" | "8017" | "8018" => {
            let length = match ai {
                "00" | "8017" | "8018" => 18,
                "01" | "02" | "03" => 14,
                "402" => 17,
                _ => 13,
            };
            value.len() == length && gs1_check_digit(value)
        }
        // GDTI and GCN: a 13-digit key and an optional serial component.
        "253" => keyed(13, 30),
        "255" => keyed(13, 25) && digits(13..value.len()),
        // GRAI: a zero, the 13-digit key and an optional serial component.
        "8003" => keyed(14, 30) && value.starts_with('0'),
        // ITIP: a GTIN and four piece digits.
        "8006" => value.len() == 18 && keyed(14, 18) && digits(14..18),
        _ => match ai_fixed_length(ai.as_bytes()) {
            Some(length) => value.len() == length && digits(0..length),
            None => !value.is_empty(),
        },
    }
}

fn gs1_elements(text: &str) -> Option<Content> {
    let bytes = text.as_bytes();
    let mut content = Content::new(ContentKind::Gs1);
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == 0x1d {
            at += 1;
            continue;
        }
        let length = ai_length(&bytes[at..])?;
        let ai = &text[at..at + length];
        at += length;
        let end = match ai_fixed_length(ai.as_bytes()) {
            Some(n) if at + n <= bytes.len() => at + n,
            Some(_) => return None,
            None => bytes[at..]
                .iter()
                .position(|&b| b == 0x1d)
                .map_or(bytes.len(), |p| at + p),
        };
        let value = text.get(at..end)?;
        if !gs1_element_valid(ai, value) {
            return None;
        }
        content.push(ai, value);
        at = end;
    }
    content.nonempty()
}

/// Human-readable form `(01)09506000134352(17)201225(10)ABC123`.
fn gs1_bracketed(text: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Gs1);
    let mut rest = text;
    while !rest.is_empty() {
        let inner = rest.strip_prefix('(')?;
        let close = inner.find(')')?;
        let ai = &inner[..close];
        if ai_length(ai.as_bytes()) != Some(ai.len()) {
            return None;
        }
        let after = &inner[close + 1..];
        let end = after.find('(').unwrap_or(after.len());
        let value = &after[..end];
        if !gs1_element_valid(ai, value) {
            return None;
        }
        content.push(ai, value);
        rest = &after[end..];
    }
    content.nonempty()
}

/// Primary-key AIs that can start a GS1 Digital Link path.
const DIGITAL_LINK_KEYS: [&str; 14] = [
    "00", "01", "253", "255", "401", "402", "414", "417", "8003", "8004", "8006", "8010", "8013",
    "8017",
];

/// GS1 Digital Link URI: a primary key AI and its value as path segments,
/// followed by qualifier segments and attribute AIs as query parameters.
fn digital_link(url: &str) -> Option<Content> {
    // Split URI components before inspecting path segments: a fragment can
    // contain '?', and a query can contain '/'. Neither belongs to the path.
    let resource = url.split('#').next()?;
    let after_scheme = &resource[resource.find("://")? + 3..];
    let (hierarchy, query) = after_scheme.split_once('?').unwrap_or((after_scheme, ""));
    let (_, path) = hierarchy.split_once('/')?;
    let segments: Vec<&str> = path.split('/').collect();
    let start = segments
        .iter()
        .position(|s| DIGITAL_LINK_KEYS.contains(s))?;
    let mut content = Content::new(ContentKind::Gs1);
    let pairs = &segments[start..];
    if !pairs.len().is_multiple_of(2) {
        return None;
    }
    let mut elements = Vec::new();
    for pair in pairs.chunks(2) {
        elements.push((pair[0].to_string(), percent_decode(pair[1])));
    }
    for parameter in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = parameter.split_once('=').unwrap_or((parameter, ""));
        if ai_length(key.as_bytes()) == Some(key.len()) {
            elements.push((key.to_string(), query_decode(value)));
        }
    }
    for (ai, mut value) in elements {
        // GS1 Digital Link accepts GTIN-8, GTIN-12 and GTIN-13 as the GTIN key and
        // reports them as GTIN-14.
        if ai == "01" && matches!(value.len(), 8 | 12 | 13) {
            value = format!("{value:0>14}");
        }
        if ai_length(ai.as_bytes()) != Some(ai.len()) || !gs1_element_valid(&ai, &value) {
            return None;
        }
        content.push(&ai, value);
    }
    content.push("url", url);
    Some(content)
}

// ---------------------------------------------------------------- URIs

/// Decode `%XX` escapes; `+` stays literal (outside query strings).
fn percent_decode(text: &str) -> String {
    decode_escapes(text, false)
}

/// Decode a query-string value, where `+` encodes a space.
fn query_decode(text: &str) -> String {
    decode_escapes(text, true)
}

fn decode_escapes(text: &str, plus_as_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| char::from(b).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(u8::try_from(h * 16 + l).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(if plus_as_space && bytes[i] == b'+' {
                b' '
            } else {
                bytes[i]
            });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|parameter| {
        let (k, v) = parameter.split_once('=')?;
        k.eq_ignore_ascii_case(key).then_some(v)
    })
}

/// Whether `text` starts with `http://` or `https://`, in any case.
fn has_web_scheme(text: &str) -> bool {
    let upper = text.get(..8).unwrap_or(text).to_ascii_uppercase();
    upper.starts_with("HTTP://") || upper.starts_with("HTTPS://")
}

/// A web address with an `http`/`https` scheme (or a GS1 Digital Link), or one
/// that starts with `www.` and a valid host name. Without a scheme, names such as
/// `example.com` stay plain text: they cannot be told apart from ordinary text.
fn web_url(text: &str) -> Option<Content> {
    let text = text.trim();
    if has_web_scheme(text) {
        return Some(digital_link(text).unwrap_or_else(|| single(ContentKind::Url, "url", text)));
    }
    let host = text.split(['/', '?', '#', ':']).next().unwrap_or(text);
    (text
        .get(..4)
        .is_some_and(|www| www.eq_ignore_ascii_case("www."))
        && domain_name(host, 3)
        && !text.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then(|| single(ContentKind::Url, "url", text))
}

/// A DNS host name with at least `labels` labels and an alphabetic top-level
/// domain of two or more letters.
fn domain_name(host: &str, labels: usize) -> bool {
    host.split('.').count() >= labels
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && host
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()))
}

/// Bookmark of a web address; its title is kept in payload order. A bookmarked
/// GS1 Digital Link stays GS1 content, without the title.
fn bookmark(url: &str, title: &str, title_first: bool) -> Option<Content> {
    let mut content = web_url(url)?;
    let title = title.trim();
    if content.kind == ContentKind::Url && !title.is_empty() {
        let field = Field {
            name: "title".into(),
            value: title.into(),
        };
        if title_first {
            content.fields.insert(0, field);
        } else {
            content.fields.push(field);
        }
    }
    Some(content)
}

/// `URLTO:title:url`; an `http(s)` address may follow without a title.
fn urlto(rest: &str) -> Option<Content> {
    if has_web_scheme(rest) {
        return bookmark(rest, "", true);
    }
    match rest.split_once(':') {
        Some((title, url)) => bookmark(url, title, true),
        None => bookmark(rest, "", true),
    }
}

/// `MEBKM:TITLE:…;URL:…;;`
fn mebkm(rest: &str) -> Option<Content> {
    let (mut title, mut url, mut title_first) = (String::new(), String::new(), false);
    for (key, value) in semicolon_pairs(rest) {
        match key.as_str() {
            "TITLE" => {
                title_first = url.is_empty();
                title = value;
            }
            "URL" => url = value,
            _ => {}
        }
    }
    bookmark(&url, &title, title_first)
}

/// Text that is exactly one e-mail address, such as `name@example.com`.
fn email_address(text: &str) -> Option<Content> {
    let text = text.trim();
    let (local, domain) = text.split_once('@')?;
    let local_valid = (1..=64).contains(&local.len())
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._%+-".contains(&b));
    (local_valid && domain_name(domain, 2)).then(|| single(ContentKind::Email, "to", text))
}

/// `SMTP:to:subject:body`.
fn smtp(rest: &str) -> Option<Content> {
    let mut parts = rest.splitn(3, ':');
    let to = parts.next()?.trim();
    if to.is_empty() {
        return None;
    }
    let mut content = single(ContentKind::Email, "to", to);
    content.push("subject", parts.next().unwrap_or(""));
    content.push("body", parts.next().unwrap_or(""));
    Some(content)
}

fn mailto(rest: &str) -> Option<Content> {
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut content = Content::new(ContentKind::Email);
    for address in to.split(',') {
        content.push("to", percent_decode(address.trim()));
    }
    for key in ["cc", "bcc"] {
        if let Some(value) = query_value(query, key) {
            content.push(key, query_decode(value));
        }
    }
    if let Some(subject) = query_value(query, "subject") {
        content.push("subject", query_decode(subject));
    }
    if let Some(body) = query_value(query, "body") {
        content.push("body", query_decode(body));
    }
    content.nonempty()
}

/// `KEY:value;` pairs with backslash escapes, as in `MECARD:`, `MATMSG:` and `WIFI:`.
fn semicolon_pairs(rest: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == ':' || c == ';' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if chars.next() != Some(':') {
            if chars.peek().is_none() {
                break;
            }
            continue;
        }
        let mut value = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        value.push(next);
                    }
                }
                ';' => break,
                _ => value.push(c),
            }
        }
        pairs.push((key.trim().to_ascii_uppercase(), value));
        if chars.peek().is_none() {
            break;
        }
    }
    pairs
}

fn matmsg(rest: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Email);
    for (key, value) in semicolon_pairs(rest) {
        match key.as_str() {
            "TO" => content.push("to", value),
            "SUB" => content.push("subject", value),
            "BODY" => content.push("body", value),
            _ => {}
        }
    }
    content.nonempty()
}

fn smsto(rest: &str) -> Option<Content> {
    let (number, message) = rest.split_once(':').unwrap_or((rest, ""));
    let mut content = Content::new(ContentKind::Sms);
    content.push("number", number.trim());
    content.push("message", message);
    content.nonempty()
}

fn sms_uri(rest: &str) -> Option<Content> {
    let (number, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut content = Content::new(ContentKind::Sms);
    for number in number.split(',') {
        content.push("number", percent_decode(number.trim()));
    }
    if let Some(subject) = query_value(query, "subject") {
        content.push("subject", query_decode(subject));
    }
    if let Some(body) = query_value(query, "body") {
        content.push("message", query_decode(body));
    }
    content.nonempty()
}

fn geo(rest: &str) -> Option<Content> {
    let (coordinates, query) = rest.split_once('?').unwrap_or((rest, ""));
    let coordinates = coordinates.split(';').next().unwrap_or_default();
    let mut parts = coordinates.split(',');
    let latitude: f64 = parts.next()?.trim().parse().ok()?;
    let longitude: f64 = parts.next()?.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    let mut content = Content::new(ContentKind::Geo);
    let mut values = coordinates.split(',').map(str::trim);
    content.push("latitude", values.next().unwrap_or_default());
    content.push("longitude", values.next().unwrap_or_default());
    if let Some(altitude) = values.next().filter(|a| a.parse::<f64>().is_ok()) {
        content.push("altitude", altitude);
    }
    if let Some(q) = query_value(query, "q") {
        content.push("query", query_decode(q));
    }
    Some(content)
}

fn wifi(rest: &str) -> Option<Content> {
    let pairs = semicolon_pairs(rest);
    let value = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let ssid = value("S")?;
    let mut content = Content::new(ContentKind::Wifi);
    content.push("ssid", ssid);
    content.push("security", value("T").unwrap_or_default());
    content.push("password", value("P").unwrap_or_default());
    if let Some(hidden) = value("H") {
        content.push("hidden", hidden.to_ascii_lowercase());
    }
    for (key, name) in [
        ("E", "eapMethod"),
        ("PH2", "phase2Method"),
        ("A", "anonymousIdentity"),
        ("I", "identity"),
    ] {
        if let Some(v) = value(key) {
            content.push(name, v);
        }
    }
    Some(content)
}

// ---------------------------------------------------------------- contacts

fn mecard(rest: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Contact);
    for (key, value) in semicolon_pairs(rest) {
        let name = match key.as_str() {
            "N" => {
                // MECARD names are "Last,First".
                let (last, first) = value.split_once(',').unwrap_or((&value, ""));
                let full = if first.is_empty() {
                    last.to_string()
                } else {
                    format!("{} {}", first.trim(), last.trim())
                };
                content.push("name", full);
                continue;
            }
            "ORG" => "organization",
            "TITLE" => "title",
            "TEL" | "TEL-AV" => "phone",
            "EMAIL" => "email",
            "ADR" => "address",
            "URL" => "url",
            "NOTE" => "note",
            "BDAY" => "birthday",
            "NICKNAME" => "nickname",
            _ => continue,
        };
        content.push(name, value);
    }
    content.nonempty()
}

/// `BIZCARD:N:first;X:last;T:title;C:company;A:address;B:phone;M:mobile;F:fax;E:email;;`
fn bizcard(rest: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Contact);
    let (mut first, mut last, mut name_at) = (String::new(), String::new(), None);
    for (key, value) in semicolon_pairs(rest) {
        let name = match key.as_str() {
            "N" | "X" => {
                name_at.get_or_insert(content.fields.len());
                if key == "N" {
                    first = value;
                } else {
                    last = value;
                }
                continue;
            }
            "T" => "title",
            "C" => "organization",
            "A" => "address",
            "B" | "M" | "F" => "phone",
            "E" => "email",
            _ => continue,
        };
        content.push(name, value);
    }
    let name = format!("{} {}", first.trim(), last.trim());
    if let Some(at) = name_at
        && !name.trim().is_empty()
    {
        let field = Field {
            name: "name".into(),
            value: name.trim().into(),
        };
        content.fields.insert(at, field);
    }
    content.nonempty()
}

/// Unfolded `NAME;PARAMS:value` lines of a vCard or iCalendar object.
fn content_lines(text: &str) -> Vec<(String, String)> {
    let mut unfolded: Vec<String> = Vec::new();
    for line in text.split('\n').map(|l| l.trim_end_matches('\r')) {
        if let Some(continued) = line.strip_prefix([' ', '\t'])
            && let Some(last) = unfolded.last_mut()
        {
            last.push_str(continued);
        } else {
            unfolded.push(line.to_string());
        }
    }
    unfolded
        .into_iter()
        .filter_map(|line| {
            let (head, value) = line.split_once(':')?;
            let name = head
                .split(';')
                .next()?
                .rsplit('.')
                .next()?
                .to_ascii_uppercase();
            Some((name, unescape_text(value)))
        })
        .collect()
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn vcard(text: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Contact);
    let lines = content_lines(text);
    let has_full_name = lines
        .iter()
        .any(|(name, v)| name == "FN" && !v.trim().is_empty());
    for (name, value) in lines {
        let field = match name.as_str() {
            "FN" => "name",
            "N" if !has_full_name => {
                // Family;Given;Additional;Prefix;Suffix
                let parts: Vec<&str> = value.split(';').collect();
                let ordered = [3, 1, 2, 0, 4]
                    .iter()
                    .filter_map(|&i| parts.get(i).map(|s| s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                content.push("name", ordered);
                continue;
            }
            "ORG" => {
                content.push(
                    "organization",
                    value.replace(';', ", ").trim_end_matches(", "),
                );
                continue;
            }
            "ADR" => {
                // PO box;extended;street;locality;region;postal code;country
                let parts: Vec<&str> = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                content.push("address", parts.join(", "));
                continue;
            }
            "TITLE" => "title",
            "TEL" => "phone",
            "EMAIL" => "email",
            "URL" => "url",
            "NOTE" => "note",
            "BDAY" => "birthday",
            "NICKNAME" => "nickname",
            _ => continue,
        };
        content.push(field, value);
    }
    content.nonempty()
}

fn vevent(text: &str) -> Option<Content> {
    let mut content = Content::new(ContentKind::Calendar);
    let mut inside = false;
    for (name, value) in content_lines(text) {
        match name.as_str() {
            "BEGIN" if value.eq_ignore_ascii_case("VEVENT") => inside = true,
            "END" if value.eq_ignore_ascii_case("VEVENT") => break,
            _ if !inside => {}
            "SUMMARY" => content.push("summary", value),
            "DTSTART" => content.push("start", value),
            "DTEND" => content.push("end", value),
            "DURATION" => content.push("duration", value),
            "LOCATION" => content.push("location", value),
            "DESCRIPTION" => content.push("description", value),
            "ORGANIZER" => content.push("organizer", value.trim_start_matches("mailto:")),
            "URL" => content.push("url", value),
            _ => {}
        }
    }
    content.nonempty()
}

// ---------------------------------------------------------------- payments

/// Swiss QR-bill (Swiss Implementation Guidelines QR-bill, version 2.x).
fn swiss_qr_bill(lines: &[&str]) -> Option<Content> {
    if lines.len() < 31 || !lines[1].starts_with("02") || lines[30] != "EPD" {
        return None;
    }
    let iban = lines[3].replace(' ', "");
    if !iban_valid(&iban) || !(iban.starts_with("CH") || iban.starts_with("LI")) {
        return None;
    }
    let mut content = Content::new(ContentKind::SwissQrBill);
    content.push("iban", iban);
    push_swiss_party(&mut content, "creditor", &lines[4..11]);
    content.push("amount", lines[18]);
    content.push("currency", lines[19]);
    push_swiss_party(&mut content, "debtor", &lines[20..27]);
    content.push("referenceType", lines[27]);
    content.push("reference", lines[28].replace(' ', ""));
    content.push("message", lines[29]);
    if let Some(billing) = lines.get(31) {
        content.push("billingInformation", *billing);
    }
    for alternative in lines.iter().skip(32).take(2) {
        content.push("alternativeScheme", *alternative);
    }
    Some(content)
}

/// Address type, name, two street/address lines, postal code, town, country.
fn push_swiss_party(content: &mut Content, party: &str, lines: &[&str]) {
    content.push(&format!("{party}Name"), lines[1]);
    let joined = |a: &str, b: &str| format!("{a} {b}").trim().to_string();
    let address_lines = if lines[0] == "S" {
        vec![joined(lines[2], lines[3]), joined(lines[4], lines[5])]
    } else {
        vec![lines[2].trim().to_string(), lines[3].trim().to_string()]
    };
    let address: Vec<String> = address_lines
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    content.push(&format!("{party}Address"), address.join(", "));
    content.push(&format!("{party}Country"), lines[6]);
}

/// EPC069-12 SEPA credit transfer QR code.
fn epc_payment(lines: &[&str]) -> Option<Content> {
    if lines.len() < 7 || !matches!(lines[1], "001" | "002") || !matches!(lines[3], "SCT" | "INST")
    {
        return None;
    }
    let iban = lines[6].replace(' ', "");
    if !iban_valid(&iban) {
        return None;
    }
    let mut content = Content::new(ContentKind::EpcPayment);
    content.push("bic", lines[4]);
    content.push("name", lines[5]);
    content.push("iban", iban);
    if let Some(amount) = lines.get(7).filter(|a| !a.is_empty()) {
        let split = amount.find(|c: char| c.is_ascii_digit()).unwrap_or(0);
        content.push("currency", &amount[..split]);
        content.push("amount", &amount[split..]);
    }
    for (index, name) in [
        (8, "purpose"),
        (9, "reference"),
        (10, "text"),
        (11, "information"),
    ] {
        if let Some(value) = lines.get(index) {
            content.push(name, *value);
        }
    }
    Some(content)
}

/// ISO 13616 mod-97 check.
fn iban_valid(iban: &str) -> bool {
    if !(15..=34).contains(&iban.len())
        || !iban
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        || !iban.as_bytes()[..2].iter().all(u8::is_ascii_uppercase)
    {
        return false;
    }
    let rotated = iban[4..].bytes().chain(iban[..4].bytes());
    let mut remainder = 0u32;
    for b in rotated {
        let value = if b.is_ascii_digit() {
            u32::from(b - b'0')
        } else {
            u32::from(b - b'A') + 10
        };
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    remainder == 1
}

// ---------------------------------------------------------------- HIBC

const HIBC_ALPHABET: &[u8; 43] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";

fn hibc_check(data: &[u8]) -> Option<u8> {
    let mut sum = 0usize;
    for b in data {
        sum += HIBC_ALPHABET.iter().position(|c| c == b)?;
    }
    Some(HIBC_ALPHABET[sum % 43])
}

/// ANSI/HIBC 2.6 supplier labeling: primary data (labeler, product, unit of
/// measure), secondary data (quantity, expiry, lot or serial) or both
/// concatenated with `/`. The final character is the mod-43 check character.
fn hibc(text: &str) -> Option<Content> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 || hibc_check(&bytes[..bytes.len() - 1])? != bytes[bytes.len() - 1] {
        return None;
    }
    let body = &text[1..text.len() - 1];
    let mut content = Content::new(ContentKind::Hibc);
    if let Some(secondary) = body.strip_prefix('$') {
        // A separate secondary symbol ends with the primary's check character as link.
        let (data, link) = secondary.split_at(secondary.len().checked_sub(1)?);
        hibc_secondary(&mut content, data, true)?;
        content.push("link", link);
        return Some(content);
    }
    let (primary, secondary) = body.split_once('/').unwrap_or((body, ""));
    if primary.len() < 6 || !primary.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let unit = &primary[primary.len() - 1..];
    if !unit.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    content.push("labelerId", &primary[..4]);
    content.push("productId", &primary[4..primary.len() - 1]);
    content.push("unitOfMeasure", unit);
    if !secondary.is_empty() {
        match secondary.strip_prefix('$') {
            Some(data) => hibc_secondary(&mut content, data, true)?,
            None => hibc_secondary(&mut content, secondary, false)?,
        }
    }
    Some(content)
}

/// Secondary data after its leading `$` (`flagged`: `$$` lot or `$$+` serial
/// with quantity and expiry flags; otherwise `$+` serial or `$` lot only), or
/// the legacy concatenated form without `$`: a YYJJJ expiry and the lot.
fn hibc_secondary(content: &mut Content, data: &str, after_dollar: bool) -> Option<()> {
    let (serial, rest) = if let Some(rest) = data.strip_prefix("$+") {
        (true, rest)
    } else if let Some(rest) = data.strip_prefix('$') {
        (false, rest)
    } else {
        if let Some(serial) = data.strip_prefix('+').filter(|_| after_dollar) {
            content.push("serial", serial);
        } else if !after_dollar
            && data.len() >= 5
            && data.as_bytes()[..5].iter().all(u8::is_ascii_digit)
        {
            content.push("expiry", &data[..5]);
            content.push("lot", &data[5..]);
        } else {
            content.push("lot", data);
        }
        return Some(());
    };
    let key = if serial { "serial" } else { "lot" };
    let mut rest = rest;
    if rest.starts_with('8') || rest.starts_with('9') {
        let digits = if rest.starts_with('8') { 2 } else { 5 };
        let quantity = rest.get(1..=digits)?;
        if !quantity.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        content.push("quantity", quantity.trim_start_matches('0'));
        rest = &rest[1 + digits..];
    }
    let date_length = match rest.as_bytes().first() {
        Some(b'0' | b'1') => Some((0, 4)),
        Some(b'2' | b'3') => Some((1, 6)),
        Some(b'4') => Some((1, 8)),
        Some(b'5') => Some((1, 5)),
        Some(b'6') => Some((1, 7)),
        Some(b'7') => Some((1, 0)),
        _ => None,
    };
    if let Some((flag, length)) = date_length {
        let expiry = rest.get(flag..flag + length)?;
        if !expiry.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        content.push("expiry", expiry);
        rest = &rest[flag + length..];
    }
    content.push(key, rest);
    Some(())
}

// ---------------------------------------------------------------- ISBN

/// An ISBN as its 13 barcode digits or written out: digits separated by single
/// hyphens or spaces, after `ISBN`, `ISBN-13:` or `ISBN-10:`, or as `urn:isbn:`.
/// Only a labelled number may use the 10-digit form; a bare 10-digit number is
/// too often something else. 979-0 numbers are printed music (ISMN), not books.
fn isbn(text: &str) -> Option<Content> {
    let text = text.trim();
    let label = text.get(..9).unwrap_or(text).to_ascii_uppercase();
    let (number, labelled) = if label.starts_with("URN:ISBN:") {
        (&text[9..], true)
    } else if label.starts_with("ISBN") {
        let rest = &text[4..];
        let rest = rest
            .strip_prefix("-13")
            .or_else(|| rest.strip_prefix("-10"))
            .unwrap_or(rest);
        (rest.strip_prefix(':').unwrap_or(rest).trim_start(), true)
    } else {
        (text, false)
    };
    let digits = isbn_characters(number)?;
    let isbn13 = match digits.len() {
        13 => String::from_utf8(digits).ok()?,
        10 if labelled && isbn10_check(&digits[..9]) == char::from(digits[9]) => {
            let mut isbn13 = String::from("978");
            isbn13.extend(digits[..9].iter().map(|&d| char::from(d)));
            let sum: u32 = isbn13
                .bytes()
                .enumerate()
                .map(|(i, d)| u32::from(d - b'0') * if i % 2 == 0 { 1 } else { 3 })
                .sum();
            isbn13.push(check_char((10 - sum % 10) % 10));
            isbn13
        }
        _ => return None,
    };
    let book =
        isbn13.starts_with("978") || (isbn13.starts_with("979") && !isbn13.starts_with("9790"));
    if !book || !gs1_check_digit(&isbn13) {
        return None;
    }
    let mut content = single(ContentKind::Isbn, "isbn13", &isbn13);
    if isbn13.starts_with("978") {
        let nine = &isbn13.as_bytes()[3..12];
        content.push(
            "isbn10",
            format!("{}{}", &isbn13[3..12], isbn10_check(nine)),
        );
    }
    Some(content)
}

/// Digits of a written ISBN, with an `X` allowed as its last character and single
/// hyphens or spaces between characters.
fn isbn_characters(number: &str) -> Option<Vec<u8>> {
    let bytes = number.as_bytes();
    let mut digits = Vec::with_capacity(13);
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'0'..=b'9' => digits.push(b),
            b'X' | b'x' if i + 1 == bytes.len() => digits.push(b'X'),
            b'-' | b' ' if i > 0 && i + 1 < bytes.len() && bytes[i - 1].is_ascii_alphanumeric() => {
            }
            _ => return None,
        }
    }
    Some(digits)
}

/// ISBN-10 check character of nine digits (mod 11, `X` for ten).
fn isbn10_check(nine: &[u8]) -> char {
    let sum: u32 = nine
        .iter()
        .zip((2..=10).rev())
        .map(|(&d, weight)| u32::from(d - b'0') * weight)
        .sum();
    match (11 - sum % 11) % 11 {
        10 => 'X',
        n => check_char(n),
    }
}

/// The digit character of a check value below ten.
fn check_char(value: u32) -> char {
    char::from_digit(value, 10).expect("check values are reduced below ten")
}

// ---------------------------------------------------------------- VIN

/// Position weights of the VIN check digit.
const VIN_WEIGHTS: [u32; 17] = [8, 7, 6, 5, 4, 3, 2, 10, 0, 9, 8, 7, 6, 5, 4, 3, 2];

/// ISO 3779 VIN with the North American check digit (position 9). Code 39
/// labels sometimes prefix an `I` (import) that is not part of the VIN.
fn vin(text: &str) -> Option<Content> {
    let vin = match text.len() {
        17 => text,
        18 if text.starts_with('I') => &text[1..],
        _ => return None,
    };
    let mut sum = 0;
    for (b, weight) in vin.bytes().zip(VIN_WEIGHTS) {
        let value = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'A'..=b'H' => u32::from(b - b'A') + 1,
            b'J'..=b'N' => u32::from(b - b'J') + 1,
            b'P' => 7,
            b'R'..=b'Z' => u32::from(b - b'R') + 2,
            _ => return None,
        };
        sum += value * weight;
    }
    let check = match sum % 11 {
        10 => b'X',
        n => b'0' + u8::try_from(n).ok()?,
    };
    if vin.as_bytes()[8] != check
        || !vin.bytes().any(|b| b.is_ascii_digit())
        || !vin.bytes().any(|b| b.is_ascii_uppercase())
    {
        return None;
    }
    let mut content = Content::new(ContentKind::Vin);
    content.push("vin", vin);
    content.push("wmi", &vin[..3]);
    content.push("vds", &vin[3..9]);
    content.push("vis", &vin[9..]);
    content.push("modelYear", &vin[9..10]);
    content.push("plant", &vin[10..11]);
    content.push("serial", &vin[11..]);
    Some(content)
}

// ---------------------------------------------------------------- AAMVA

/// AAMVA DL/ID card design standard (PDF417 on North American licenses).
fn aamva(text: &str) -> Option<Content> {
    let header = text.find("ANSI ").or_else(|| text.find("AAMVA"))?;
    let after = &text[header + 5..];
    let iin = after.get(..6)?;
    let version = after.get(6..8)?;
    if !iin.bytes().all(|b| b.is_ascii_digit()) || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut content = Content::new(ContentKind::Aamva);
    content.push("issuerId", iin);
    content.push("version", version.parse::<u32>().ok()?.to_string());
    // Subfiles start with their two-letter type (DL, ID, ...) directly before
    // the first three-letter element; offsets in the header are unreliable in
    // practice, so element lines are recognized by their identifiers.
    let mut document = false;
    for line in text.split(['\n', '\r']) {
        let mut line = line.trim_start_matches('\u{1e}');
        // The subfile type sits between the header and the first element.
        if let Some(position) = [
            "DLDAQ", "DLDCA", "DLDCS", "DLDAA", "IDDAQ", "IDDCS", "IDDAA",
        ]
        .iter()
        .filter_map(|marker| line.find(marker))
        .min()
        {
            if !document {
                document = true;
                content.push("documentType", &line[position..position + 2]);
            }
            line = &line[position + 2..];
        }
        let Some(id) = line.get(..3) else { continue };
        if !id.bytes().all(|b| b.is_ascii_uppercase()) || !id.starts_with(['D', 'Z']) {
            continue;
        }
        let value = line[3..].trim();
        let name = match id {
            "DAQ" => "licenseNumber",
            "DCS" | "DAB" => "lastName",
            "DAC" | "DCT" => "firstName",
            "DAD" => "middleName",
            "DAA" => "fullName",
            "DBB" => "dateOfBirth",
            "DBA" => "expirationDate",
            "DBD" => "issueDate",
            "DBC" => "sex",
            "DAG" => "street",
            "DAH" => "street2",
            "DAI" => "city",
            "DAJ" => "state",
            "DAK" => "postalCode",
            "DCG" => "country",
            "DAU" => "height",
            "DAY" => "eyeColor",
            "DAZ" => "hairColor",
            "DCA" => "vehicleClass",
            "DCB" => "restrictions",
            "DCD" => "endorsements",
            "DCF" => "documentDiscriminator",
            "DDA" => "complianceType",
            _ => continue,
        };
        content.push(name, value);
    }
    content
        .get("licenseNumber")
        .or_else(|| content.get("lastName"))
        .is_some()
        .then_some(content)
}

#[cfg(test)]
mod tests;
