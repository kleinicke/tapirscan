# Structured content

Content parsing returns the named fields of structured barcode text: GS1
element strings, web addresses, Wi-Fi settings, contacts, payment slips,
health-industry labels, VINs, book numbers and driver's licenses. It runs on decoded text only and never
changes what is scanned. Plain text has no content.

```python
content = barcode.content  # or tapirscan.parse_content(text)
if content and content.kind == "WiFi":
    connect(content.get("ssid"), content.get("password"))
```

| Language   | From a barcode                                    | From text                                     |
| ---------- | ------------------------------------------------- | --------------------------------------------- |
| Rust       | `barcode.content()`                               | `tapirscan::parse_content(text, gs1)`         |
| Python     | `barcode.content`, `parse_content(barcode)`       | `tapirscan.parse_content(text, *, gs1=None)`  |
| JavaScript | `barcode.content`                                 | none                                          |
| Java       | `Tapirscan.parseContent(barcode)`                 | `Tapirscan.parseContent(text)`, `(text, gs1)` |
| C++        | `tapirscan::parse_content(barcode)`               | `tapirscan::parse_content(text, gs1 = false)` |
| C          | `tapirscan_parse_content` with the barcode's flag | `tapirscan_parse_content` with `gs1` 0 or 1   |
| Swift      | `parseContent(barcode)`, `barcode.content`        | `parseContent(text, gs1: false)`              |
| Android    | `Tapirscan.parseContent(barcode)`                 | `Tapirscan.parseContent(text)`, `(text, gs1)` |

Plain text has no content: an empty optional, `None`, `nil`, an absent
`content` field or a 0 handle (see the
[C guide](../bindings/c/README.md#structured-content)); Swift calls throw native
errors. Where `gs1` is optional, text is parsed without GS1 and a barcode with
its own flag; Python's `gs1` overrides the flag when given.

A `Content` has a `kind` and `fields` in payload order. Each field is a
`name`/`value` pair; a name repeats when the payload repeats it, such as a
contact's phone numbers. `get(name)` returns the first value, `get_all(name)`
every value (`getAll` in Java and Android, `all` in Swift). Values are
unescaped but otherwise as encoded: dates, amounts and coordinates stay text.

GS1 element strings carry no visible marker, so they are parsed only when the
reader reported GS1 or you pass `gs1 = true`.

## JSON

Wherever a barcode is exported as JSON, `content` sits beside its other fields:
Rust serde, the JSON of a C scan result, Python `as_dict()` and the JavaScript
barcode. The C++ and Java accessors read that JSON only for inspection
diagnostics, so they expose no `content` field; use `parse_content`. It has the
same shape everywhere it appears:

```json
{ "kind": "WiFi", "fields": [{ "name": "ssid", "value": "Cafe" }] }
```

Plain text has no `content` member (Python `as_dict()` has `None`, like its other
unavailable metadata). Native typed results parse the text when the JSON is
produced, so their scans do not pay for it. JavaScript and Python barcodes carry
`content` from the scan, which costs a few microseconds per decoded barcode.
Inspection reports are unstable engine diagnostics and carry no `content`.

## Kinds

Kinds marked _experimental_ may change their fields or recognition, or be
removed, in minor releases. The other kinds and their field names are stable.

| Kind                         | Recognized by                                                      |
| ---------------------------- | ------------------------------------------------------------------ |
| `GS1`                        | GS1 flag; `]C1`, `]d2`, `]Q3`, `]e0`, `]J1`; `(01)…`; Digital Link |
| `URL`                        | `http://`, `https://`, `www.` with a host name; `URLTO:`, `MEBKM:` |
| `Email`                      | `mailto:`, `MATMSG:`, `SMTP:`; text that is one e-mail address     |
| `Phone`                      | `tel:`                                                             |
| `SMS`                        | `SMSTO:`, `MMSTO:`, `sms:`, `mms:`                                 |
| `Geo`                        | `geo:`                                                             |
| `WiFi`                       | `WIFI:`                                                            |
| `Contact`                    | vCard, `MECARD:`, `BIZCARD:`                                       |
| `Calendar`                   | iCalendar `VEVENT`                                                 |
| `ISBN`                       | 978 and 979 (not 979-0) EAN-13 digits, `ISBN …`, `urn:isbn:`       |
| `SwissQRBill` (experimental) | `SPC` with an `EPD` trailer                                        |
| `EPCPayment` (experimental)  | SEPA credit transfer, `BCD`                                        |
| `HIBC` (experimental)        | `+` and a valid mod-43 check character                             |
| `VIN` (experimental)         | 17 characters with a valid check digit                             |
| `AAMVA` (experimental)       | `@` and an `ANSI ` or `AAMVA` header                               |

Without a scheme, only `www.` addresses are URLs: names such as `example.com`
cannot be told apart from ordinary text, so they stay plain. The `URL` field of a
`MEBKM:` bookmark is the exception: it may name such a host without a scheme.

## Fields

- **GS1:** one field per element, named by its application identifier (`01`,
  `17`, `10`, `3103`, …). Every form applies the same element rules:
  fixed-length values have exactly their length and identification keys (GTIN,
  SSCC, GLN, …) a valid check digit, so an invalid element makes the text plain
  (or a URL). Digital Link URLs add `url` and report a GTIN-8, -12 or -13 key
  as 14 digits.
- **URL:** `url` as encoded (a `www.` address has no scheme), and `title` for
  bookmarks. **Phone:** `number`.
- **Email:** `to` (repeated), `cc`, `bcc`, `subject`, `body`.
- **SMS:** `number` (repeated), `subject`, `message`.
- **Geo:** `latitude`, `longitude`, `altitude`, `query`.
- **WiFi:** `ssid`, `security` (`WPA`, `WEP`, `nopass`, …), `password`, `hidden`,
  and for enterprise networks `eapMethod`, `phase2Method`, `anonymousIdentity`,
  `identity`.
- **Contact:** `name`, `organization`, `title`, `phone`, `email`, `address`,
  `url`, `note`, `birthday`, `nickname`.
- **Calendar:** `summary`, `start`, `end`, `duration`, `location`,
  `description`, `organizer`, `url`.
- **SwissQRBill:** `iban`, `creditorName`, `creditorAddress`,
  `creditorCountry`, `amount`, `currency`, the same three `debtor…` fields,
  `referenceType`, `reference`, `message`, `billingInformation`,
  `alternativeScheme`.
- **EPCPayment:** `bic`, `name`, `iban`, `currency`, `amount`, `purpose`,
  `reference`, `text`, `information`.
- **HIBC:** `labelerId`, `productId`, `unitOfMeasure`; secondary data
  `quantity`, `expiry`, `lot` or `serial`; `link` on a separate secondary symbol.
- **VIN:** `vin`, `wmi`, `vds`, `vis`, `modelYear`, `plant`, `serial`. An `I`
  prefix on Code 39 labels is removed.
- **AAMVA:** `issuerId`, `version`, `documentType` (`DL`, `ID`),
  `licenseNumber`, `lastName`, `firstName`, `middleName`, `fullName`,
  `dateOfBirth`, `expirationDate`, `issueDate`, `sex`, `street`, `street2`,
  `city`, `state`, `postalCode`, `country`, `height`, `eyeColor`, `hairColor`,
  `vehicleClass`, `restrictions`, `endorsements`, `documentDiscriminator`,
  `complianceType`. Dates keep the issuer's order (`MMDDCCYY` in the US,
  `CCYYMMDD` in Canada).
- **ISBN:** `isbn13`, and `isbn10` for numbers starting with 978. A book's price
  is not in the number; many books print it in a five-digit supplement, by a
  convention that varies by country. Read it as `eanAddOn` with the
  `eanAddOnPolicy` `read`.

## Validation

A payload that fails these checks has no content, rather than partial fields:

- **GS1:** known application identifiers, fixed-length elements of the right
  length, and GTIN/SSCC/GLN check digits.
- **Payments:** the IBAN checksum; Swiss QR-bills also need a Swiss or
  Liechtenstein IBAN and all mandatory lines.
- **HIBC:** the check character. **VIN:** the position-9 check digit, so VINs
  without one (common in Europe) are not recognized.
- **AAMVA:** a license number or last name.
- **URL:** a host after `http://` or `https://`, and no whitespace in the
  address.
- **ISBN:** the check digit. A 10-digit ISBN needs the `ISBN` or `urn:isbn:`
  label, a label that names the length (`ISBN-13:`, `ISBN10:`) needs that many
  digits, and 979-0 numbers are printed music (ISMN), not books.

Not covered: Aadhaar QR codes, GS1 element titles and value conversion, Swiss
QR-bill alternative-scheme payloads, and vCard parameters such as `TYPE=WORK`.
