use super::*;

fn fields(content: &Content) -> Vec<(&str, &str)> {
    content
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.value.as_str()))
        .collect()
}

fn parsed(text: &str, gs1: bool) -> (ContentKind, Vec<(String, String)>) {
    let content = parse_content(text, gs1).unwrap_or_else(|| panic!("no content: {text:?}"));
    let fields = fields(&content)
        .into_iter()
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect();
    (content.kind, fields)
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
        .collect()
}

#[test]
fn gs1_element_strings() {
    let expected = pairs(&[
        ("01", "09506000134352"),
        ("17", "201225"),
        ("10", "ABC123"),
        ("21", "XYZ"),
    ]);
    let raw = "010950600013435217201225".to_string() + "10ABC123\u{1d}21XYZ";
    assert_eq!(parsed(&raw, true), (ContentKind::Gs1, expected.clone()));
    assert_eq!(parsed(&format!("]C1{raw}"), false).1, expected);
    assert_eq!(
        parsed("(01)09506000134352(17)201225(10)ABC123(21)XYZ", false).1,
        expected
    );
    // Four-digit AIs, a separator after a fixed-length element, SSCC.
    assert_eq!(
        parsed("3103000150\u{1d}00106141411234567897", true).1,
        pairs(&[("3103", "000150"), ("00", "106141411234567897")])
    );
    // A wrong GTIN check digit, an unknown AI and a short fixed field fail.
    for bad in ["0109506000134353", "2712345", "01095060001343"] {
        assert_eq!(parse_content(bad, true), None, "{bad}");
    }
    // Keys with a serial part check the key; fixed-length bracketed values need their length.
    assert_eq!(
        parsed("(253)9506000000008ABC(8006)095060001343520102", false).1,
        pairs(&[("253", "9506000000008ABC"), ("8006", "095060001343520102")])
    );
    for bad in [
        "(253)9506000000009ABC",
        "(255)9506000000008X",
        "(8006)0950600013435201",
        "(8003)19506000000008",
        "(402)9506000000008",
        "(17)2012",
        "(01)",
    ] {
        assert_eq!(parse_content(bad, false), None, "{bad}");
    }
    assert!(!gs1_check_digit(""));
    // A GS1-flagged Digital Link URL is still recognized.
    assert_eq!(
        parsed("https://id.gs1.org/01/09506000134352", true).1[0],
        ("01".into(), "09506000134352".into())
    );
    // Without the flag, plain digits are not GS1.
    assert_eq!(parse_content("0109506000134352", false), None);
}

#[test]
fn gs1_digital_link() {
    let (kind, values) = parsed(
        "https://id.gs1.org/01/09506000134352/10/ABC%20123?17=201225&utm=x",
        false,
    );
    assert_eq!(kind, ContentKind::Gs1);
    assert_eq!(
        values[..3],
        pairs(&[
            ("01", "09506000134352"),
            ("10", "ABC 123"),
            ("17", "201225")
        ])
    );
    assert_eq!(values[3].0, "url");
    // A 13-digit GTIN is zero-padded to 14 digits; other URLs stay URLs.
    assert_eq!(
        parsed("https://example.com/01/9506000134352", false).1[0],
        ("01".into(), "09506000134352".into())
    );
    // GTIN-8 and GTIN-12 are padded as well; SSCC and GLN keys are not.
    for (gtin, padded) in [
        ("96385074", "00000096385074"),
        ("036000291452", "00036000291452"),
    ] {
        assert_eq!(
            parsed(&format!("https://example.com/01/{gtin}"), false).1[0],
            ("01".into(), padded.into())
        );
    }
    assert_eq!(
        parsed("https://example.com/414/9506000000008", false).1[0],
        ("414".into(), "9506000000008".into())
    );
    // Digital Link follows the element rules: empty values, other lengths, wrong
    // check digits and malformed attributes leave a plain URL.
    for url in [
        "https://example.com/01/",
        "https://example.com/01/0",
        "https://example.com/01/3600029145",
        "https://example.com/01/009506000134352",
        "https://example.com/01/09506000134353",
        "https://example.com/00/10614141123456789",
        "https://example.com/00/0106141411234567897",
        "https://example.com/414/950600000000",
        "https://example.com/414/09506000000008",
        "https://example.com/01/09506000134352/10/",
        "https://example.com/01/09506000134352?17=2012",
        "https://example.com/01/09506000134352?17=",
    ] {
        assert_eq!(parsed(url, false).0, ContentKind::Url, "{url}");
    }
    assert_eq!(
        parsed("https://example.com/shop/item?id=1", false),
        (
            ContentKind::Url,
            pairs(&[("url", "https://example.com/shop/item?id=1")])
        )
    );
}

#[test]
fn gs1_digital_link_respects_uri_boundaries() {
    for (suffix, fields) in [
        ("?17=201225#details", vec![("17", "201225")]),
        ("?10=LOT#details", vec![("10", "LOT")]),
        ("#details?17=201225", vec![]),
        ("?10=LOT%23A#details", vec![("10", "LOT#A")]),
    ] {
        let url = format!("https://example.com/01/09506000134352{suffix}");
        let content = parse_content(&url, false).unwrap();
        assert_eq!(content.kind, ContentKind::Gs1, "{url}");
        let mut expected = vec![("01", "09506000134352")];
        expected.extend(fields);
        expected.push(("url", &url));
        assert_eq!(
            content
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.value.as_str()))
                .collect::<Vec<_>>(),
            expected,
            "{url}"
        );
    }
    // A path embedded in a query or fragment is not the URL's own path.
    for url in [
        "https://example.com?next=/01/09506000134352",
        "https://example.com#/01/09506000134352",
    ] {
        assert_eq!(parsed(url, false).0, ContentKind::Url, "{url}");
    }
}

#[test]
fn messages_and_locations() {
    assert_eq!(
        parsed(
            "mailto:ana@example.com?subject=Hi%20there&body=See+you",
            false
        ),
        (
            ContentKind::Email,
            pairs(&[
                ("to", "ana@example.com"),
                ("subject", "Hi there"),
                ("body", "See you")
            ])
        )
    );
    assert_eq!(
        parsed(
            "MATMSG:TO:ana@example.com;SUB:Hi;BODY:Text\\; more;;",
            false
        )
        .1,
        pairs(&[
            ("to", "ana@example.com"),
            ("subject", "Hi"),
            ("body", "Text; more")
        ])
    );
    assert_eq!(
        parsed("tel:+41 44 668 18 00", false),
        (ContentKind::Phone, pairs(&[("number", "+41 44 668 18 00")]))
    );
    assert_eq!(
        parsed("SMSTO:+4915112345678:Hello: world", false).1,
        pairs(&[("number", "+4915112345678"), ("message", "Hello: world")])
    );
    assert_eq!(
        parsed("sms:+4915112345678?body=Hi%21", false).1,
        pairs(&[("number", "+4915112345678"), ("message", "Hi!")])
    );
    assert_eq!(
        parsed("geo:52.5163,13.3777,34?q=Brandenburger+Tor", false),
        (
            ContentKind::Geo,
            pairs(&[
                ("latitude", "52.5163"),
                ("longitude", "13.3777"),
                ("altitude", "34"),
                ("query", "Brandenburger Tor")
            ])
        )
    );
    assert_eq!(parse_content("geo:95,10", false), None);
}

#[test]
fn wifi_networks() {
    assert_eq!(
        parsed("WIFI:T:WPA;S:Cafe\\;Guest;P:pa\\:ss\\\\1;H:TRUE;;", false),
        (
            ContentKind::Wifi,
            pairs(&[
                ("ssid", "Cafe;Guest"),
                ("security", "WPA"),
                ("password", "pa:ss\\1"),
                ("hidden", "true")
            ])
        )
    );
    // Key order is free; open networks have no password.
    assert_eq!(
        parsed("WIFI:S:Open;T:nopass;;", false).1,
        pairs(&[("ssid", "Open"), ("security", "nopass")])
    );
    assert_eq!(parse_content("WIFI:T:WPA;P:x;;", false), None);
}

#[test]
fn contacts_and_events() {
    let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Muster;Erika;;Dr.;\r\nFN:Dr. Erika Muster\r\n\
                 ORG:Example AG;Research\r\nTITLE:Lead\r\nTEL;TYPE=WORK:+49 30 1234\r\n\
                 TEL;TYPE=CELL:+49 170 5678\r\nEMAIL:erika@example.com\r\n\
                 ADR;TYPE=WORK:;;Main St 1;Berlin;;10115;Germany\r\nNOTE:Line one\\nline\r\n  two\r\n\
                 END:VCARD";
    assert_eq!(
        parsed(vcard, false),
        (
            ContentKind::Contact,
            pairs(&[
                ("name", "Dr. Erika Muster"),
                ("organization", "Example AG, Research"),
                ("title", "Lead"),
                ("phone", "+49 30 1234"),
                ("phone", "+49 170 5678"),
                ("email", "erika@example.com"),
                ("address", "Main St 1, Berlin, 10115, Germany"),
                ("note", "Line one\nline two")
            ])
        )
    );
    let content = parse_content(vcard, false).unwrap();
    assert_eq!(content.get_all("phone").count(), 2);
    assert_eq!(content.get("email"), Some("erika@example.com"));
    // Without FN, N is ordered prefix, given, additional, family.
    assert_eq!(
        parsed("BEGIN:VCARD\nN:Muster;Erika\nEND:VCARD", false).1,
        pairs(&[("name", "Erika Muster")])
    );
    assert_eq!(
        parsed(
            "MECARD:N:Muster,Erika;TEL:+4930;EMAIL:e@x.de;URL:https://x.de;;",
            false
        )
        .1,
        pairs(&[
            ("name", "Erika Muster"),
            ("phone", "+4930"),
            ("email", "e@x.de"),
            ("url", "https://x.de")
        ])
    );
    assert_eq!(
        parsed(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:Launch\\, party\nDTSTART:20261024T180000Z\n\
             DTEND:20261024T220000Z\nLOCATION:Hall 2\nEND:VEVENT\nEND:VCALENDAR",
            false
        ),
        (
            ContentKind::Calendar,
            pairs(&[
                ("summary", "Launch, party"),
                ("start", "20261024T180000Z"),
                ("end", "20261024T220000Z"),
                ("location", "Hall 2")
            ])
        )
    );
}

fn swiss_bill(iban: &str) -> String {
    [
        "SPC",
        "0200",
        "1",
        iban,
        "S",
        "Robert Schneider AG",
        "Rue du Lac",
        "1268",
        "2501",
        "Biel",
        "CH",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "1949.75",
        "CHF",
        "K",
        "Pia-Maria Rutschmann-Schnyder",
        "Grosse Marktgasse 28",
        "9400 Rorschach",
        "",
        "",
        "CH",
        "QRR",
        "21 00000 00003 13947 14300 09017",
        "Order of 15 June 2020",
        "EPD",
        "//S1/10/10201409/11/200701/20/140.000-53",
    ]
    .join("\r\n")
}

#[test]
fn payment_slips() {
    assert_eq!(
        parsed(&swiss_bill("CH44 3199 9123 0008 8901 2"), false),
        (
            ContentKind::SwissQrBill,
            pairs(&[
                ("iban", "CH4431999123000889012"),
                ("creditorName", "Robert Schneider AG"),
                ("creditorAddress", "Rue du Lac 1268, 2501 Biel"),
                ("creditorCountry", "CH"),
                ("amount", "1949.75"),
                ("currency", "CHF"),
                ("debtorName", "Pia-Maria Rutschmann-Schnyder"),
                ("debtorAddress", "Grosse Marktgasse 28, 9400 Rorschach"),
                ("debtorCountry", "CH"),
                ("referenceType", "QRR"),
                ("reference", "210000000003139471430009017"),
                ("message", "Order of 15 June 2020"),
                (
                    "billingInformation",
                    "//S1/10/10201409/11/200701/20/140.000-53"
                )
            ])
        )
    );
    // A corrupted IBAN or a missing trailer is not a QR-bill.
    assert_eq!(
        parse_content(&swiss_bill("CH4431999123000889013"), false),
        None
    );
    assert_eq!(
        parse_content(
            &swiss_bill("CH4431999123000889012").replace("EPD", "XXX"),
            false
        ),
        None
    );
    let epc = "BCD\n002\n1\nSCT\nBHBLDEHHXXX\nFranz Mustermann\nDE71 1102 2033 0123 4567 89\n\
               EUR12.30\nGDDS\nRF18539007547034\n\nThanks";
    assert_eq!(
        parsed(epc, false),
        (
            ContentKind::EpcPayment,
            pairs(&[
                ("bic", "BHBLDEHHXXX"),
                ("name", "Franz Mustermann"),
                ("iban", "DE71110220330123456789"),
                ("currency", "EUR"),
                ("amount", "12.30"),
                ("purpose", "GDDS"),
                ("reference", "RF18539007547034"),
                ("information", "Thanks")
            ])
        )
    );
    assert_eq!(parse_content(&epc.replace("SCT", "XYZ"), false), None);
}

fn hibc_symbol(data: &str) -> String {
    let check = hibc_check(data.as_bytes()).expect("HIBC alphabet");
    format!("{data}{}", char::from(check))
}

#[test]
fn hibc_labels() {
    assert_eq!(
        parsed(&hibc_symbol("+A99912345"), false),
        (
            ContentKind::Hibc,
            pairs(&[
                ("labelerId", "A999"),
                ("productId", "1234"),
                ("unitOfMeasure", "5")
            ])
        )
    );
    // Concatenated primary and secondary: Julian expiry (flag 5) and lot.
    assert_eq!(
        parsed(&hibc_symbol("+A99912345/$$52001510X"), false).1,
        pairs(&[
            ("labelerId", "A999"),
            ("productId", "1234"),
            ("unitOfMeasure", "5"),
            ("expiry", "20015"),
            ("lot", "10X")
        ])
    );
    // Separate secondary symbol with quantity, YYMMDD expiry, serial and link character.
    assert_eq!(
        parsed(&hibc_symbol("+$$+8243331012LOT9X"), false).1,
        pairs(&[
            ("quantity", "24"),
            ("expiry", "331012"),
            ("serial", "LOT9"),
            ("link", "X")
        ])
    );
    // Lot-only and serial-only secondary symbols keep digits in the value.
    assert_eq!(
        parsed(&hibc_symbol("+$20150LOTX"), false).1,
        pairs(&[("lot", "20150LOT"), ("link", "X")])
    );
    assert_eq!(
        parsed(&hibc_symbol("+$+SN42X"), false).1,
        pairs(&[("serial", "SN42"), ("link", "X")])
    );
    assert_eq!(parse_content("+A999123450", false), None);
}

#[test]
fn vehicle_identification_numbers() {
    assert_eq!(
        parsed("1M8GDM9AXKP042788", false),
        (
            ContentKind::Vin,
            pairs(&[
                ("vin", "1M8GDM9AXKP042788"),
                ("wmi", "1M8"),
                ("vds", "GDM9AX"),
                ("vis", "KP042788"),
                ("modelYear", "K"),
                ("plant", "P"),
                ("serial", "042788")
            ])
        )
    );
    assert_eq!(
        parsed("I1M8GDM9AXKP042788", false).1[0].1,
        "1M8GDM9AXKP042788"
    );
    // Wrong check digit, forbidden letters and plain 17-digit numbers are not VINs.
    for bad in [
        "1M8GDM9A1KP042788",
        "1M8GDM9AXKP04278O",
        "12345678901234567",
    ] {
        assert_eq!(parse_content(bad, false), None, "{bad}");
    }
}

#[test]
fn aamva_licenses() {
    let license = "@\n\u{1e}\rANSI 636014040002DL00410278ZC03190024DLDAQD1234562\nDCSPUBLIC\n\
                   DACJOHN\nDADQ\nDBB01011990\nDBA01012030\nDBC1\nDAG123 MAIN ST\nDAISACRAMENTO\n\
                   DAJCA\nDAK958220000  \nDCGUSA\nDAYBRO\r";
    assert_eq!(
        parsed(license, false),
        (
            ContentKind::Aamva,
            pairs(&[
                ("issuerId", "636014"),
                ("version", "4"),
                ("documentType", "DL"),
                ("licenseNumber", "D1234562"),
                ("lastName", "PUBLIC"),
                ("firstName", "JOHN"),
                ("middleName", "Q"),
                ("dateOfBirth", "01011990"),
                ("expirationDate", "01012030"),
                ("sex", "1"),
                ("street", "123 MAIN ST"),
                ("city", "SACRAMENTO"),
                ("state", "CA"),
                ("postalCode", "958220000"),
                ("country", "USA"),
                ("eyeColor", "BRO")
            ])
        )
    );
    assert_eq!(parse_content("@\nANSI 6360140400", false), None);
}

#[test]
fn unstructured_text_has_no_content() {
    for text in [
        "",
        "Hello",
        "4006381333931",
        "9780306406158",
        "9790260000438",
        "ABC-123",
        "+",
        "BCD",
        "SPC\n0200",
    ] {
        assert_eq!(parse_content(text, false), None, "{text:?}");
    }
    assert_eq!(ContentKind::ALL.len(), 15);
    for (i, kind) in ContentKind::ALL.iter().enumerate() {
        assert_eq!(*kind as usize, i + 1);
        assert_eq!(
            serde_json::to_value(kind).unwrap(),
            serde_json::json!(kind.as_str())
        );
    }
}

#[test]
fn web_addresses_without_a_scheme_and_bookmarks() {
    for (text, url) in [
        ("www.example.com", "www.example.com"),
        ("WWW.EXAMPLE.CO.UK/MENU?T=4", "WWW.EXAMPLE.CO.UK/MENU?T=4"),
        ("www.shop-1.de:8080/a#b\n", "www.shop-1.de:8080/a#b"),
        ("https://example.com/menu\r\n", "https://example.com/menu"),
    ] {
        assert_eq!(
            parsed(text, false),
            (ContentKind::Url, pairs(&[("url", url)])),
            "{text:?}"
        );
    }
    for (text, fields) in [
        (
            "URLTO:Menu:https://example.com/menu",
            [("title", "Menu"), ("url", "https://example.com/menu")].as_slice(),
        ),
        (
            "URLTO:www.shop.de:https://www.shop.de",
            &[("title", "www.shop.de"), ("url", "https://www.shop.de")],
        ),
        ("urlto::www.example.com", &[("url", "www.example.com")]),
        ("URLTO:http://example.com", &[("url", "http://example.com")]),
        (
            r"MEBKM:TITLE:Tapir\;s;URL:https://example.com/a;;",
            &[("title", "Tapir;s"), ("url", "https://example.com/a")],
        ),
        (
            "MEBKM:URL:https://example.com/a;TITLE:Menu;;",
            &[("url", "https://example.com/a"), ("title", "Menu")],
        ),
    ] {
        assert_eq!(
            parsed(text, false),
            (ContentKind::Url, pairs(fields)),
            "{text}"
        );
    }
    // A bookmarked GS1 Digital Link is GS1 content; GS1 fields have no title.
    assert_eq!(
        parsed(
            "MEBKM:TITLE:x;URL:https://id.gs1.org/01/09506000134352;;",
            false
        ),
        (
            ContentKind::Gs1,
            pairs(&[
                ("01", "09506000134352"),
                ("url", "https://id.gs1.org/01/09506000134352")
            ])
        )
    );
    for text in [
        "example.com",
        "shop.example.com/menu",
        "www.example",
        "www.example.c0m",
        "www..example.com",
        "www.-example.com",
        "www.example.com and more",
        "URLTO:Menu:example.com",
        "MEBKM:TITLE:x;URL:ftp://example.com;;",
    ] {
        assert_eq!(parse_content(text, false), None, "{text:?}");
    }
    // A text that fails as a `www.` address can still be another kind.
    assert_eq!(
        parsed("www.info@example.com", false),
        (ContentKind::Email, pairs(&[("to", "www.info@example.com")]))
    );
}

#[test]
fn bare_and_legacy_email_addresses() {
    for (text, to) in [
        (
            "first.last+tag@mail.example.org",
            "first.last+tag@mail.example.org",
        ),
        ("info@example.com\n", "info@example.com"),
    ] {
        assert_eq!(
            parsed(text, false),
            (ContentKind::Email, pairs(&[("to", to)])),
            "{text:?}"
        );
    }
    assert_eq!(
        parsed("SMTP:a@example.com:Hello:Body: with colon", false),
        (
            ContentKind::Email,
            pairs(&[
                ("to", "a@example.com"),
                ("subject", "Hello"),
                ("body", "Body: with colon")
            ])
        )
    );
    for text in [
        "a@b",
        "a@example.c",
        "a@example.123",
        ".a@example.com",
        "a..b@example.com",
        "a b@example.com",
        "a@b@example.com",
        "@example.com",
        "Mail a@example.com",
        "SMTP::Hello",
    ] {
        assert_eq!(parse_content(text, false), None, "{text:?}");
    }
}

#[test]
fn mms_links_and_business_cards() {
    assert_eq!(
        parsed("mms:+15551234?subject=Photo&body=Hi%20there", false),
        (
            ContentKind::Sms,
            pairs(&[
                ("number", "+15551234"),
                ("subject", "Photo"),
                ("message", "Hi there")
            ])
        )
    );
    assert_eq!(
        parsed(
            "BIZCARD:T:Engineer;N:Sean;X:Owen;C:Example Inc.;A:76 9th Avenue;\
             B:+12125551212;M:+12125559999;E:sean@example.com;;",
            false
        ),
        (
            ContentKind::Contact,
            pairs(&[
                ("title", "Engineer"),
                ("name", "Sean Owen"),
                ("organization", "Example Inc."),
                ("address", "76 9th Avenue"),
                ("phone", "+12125551212"),
                ("phone", "+12125559999"),
                ("email", "sean@example.com")
            ])
        )
    );
    assert_eq!(parse_content("BIZCARD:;", false), None);
}

#[test]
fn book_numbers() {
    let isbn = |isbn13: &str, isbn10: Option<&str>| {
        let mut fields = vec![("isbn13", isbn13)];
        fields.extend(isbn10.map(|isbn10| ("isbn10", isbn10)));
        (ContentKind::Isbn, pairs(&fields))
    };
    for (text, isbn13, isbn10) in [
        ("9780306406157", "9780306406157", Some("0306406152")),
        ("9780804429573", "9780804429573", Some("080442957X")),
        ("978-0-306-40615-7", "9780306406157", Some("0306406152")),
        (
            "ISBN 978 0 306 40615 7",
            "9780306406157",
            Some("0306406152"),
        ),
        (
            "ISBN-13: 978-0-306-40615-7",
            "9780306406157",
            Some("0306406152"),
        ),
        (
            "ISBN-10: 0-306-40615-2",
            "9780306406157",
            Some("0306406152"),
        ),
        ("isbn 080442957x", "9780804429573", Some("080442957X")),
        ("urn:isbn:0306406152", "9780306406157", Some("0306406152")),
        // 979 numbers have no ISBN-10.
        ("9791032305690", "9791032305690", None),
    ] {
        assert_eq!(parsed(text, false), isbn(isbn13, isbn10), "{text}");
    }
    for text in [
        // Bare 10-digit numbers are too often phone numbers or other IDs.
        "0306406152",
        // Wrong check digits.
        "9780306406158",
        "ISBN 0-306-40615-3",
        // 979-0 is the ISMN range for printed music.
        "9790260000438",
        "ISBN 979-0-2600-0043-8",
        // Malformed separators or lengths, and other EAN-13 numbers.
        "978--0306406157",
        "-9780306406157",
        "978030640615",
        "ISBN",
        "4006381333931",
    ] {
        assert_eq!(parse_content(text, false), None, "{text:?}");
    }
}
