# Structured content

Barcodes often carry structured text: GS1 element strings on products and
logistics labels, Wi-Fi settings and contact cards in QR codes, payment slips,
health-industry labels, vehicle numbers and driver's licenses. Content parsing
recognizes these payloads in decoded text and returns their named fields.

Parsing is separate from scanning. It never changes what the scanner reads, it
runs only on the text you pass (or, in JavaScript, on each decoded barcode), and
text without a recognized structure has no content. One Rust implementation
serves every binding, so all languages return the same kinds and fields.

```python
content = tapirscan.parse_content(barcode)  # a Barcode or a str
if content and content.kind == "WiFi":
    connect(content.get("ssid"), content.get("password"))
```

| Language   | Call                                                                   | Result                                      |
| ---------- | ---------------------------------------------------------------------- | ------------------------------------------- |
| Rust       | `barcode.content()`, `tapirscan::parse_content(text, gs1)`             | `Option<Content>`                           |
| Python     | `tapirscan.parse_content(barcode_or_text, gs1=None)`                   | `Content \| None`                           |
| JavaScript | `barcode.content` (filled during scanning)                             | `Content \| undefined`, plain frozen data   |
| Java       | `Tapirscan.parseContent(barcode)`, `Tapirscan.parseContent(text, gs1)` | `Optional<Content>`                         |
| C++        | `tapirscan::parse_content(barcode)`, `parse_content(text, gs1)`        | `std::optional<Content>`                    |
| C          | `tapirscan_parse_content(text, length, gs1, &handle, &error)`          | handle, 0 for no content; see `tapirscan.h` |
| Swift      | `barcode.content`, `parseContent(text, gs1:)`                          | `Content?`                                  |
| Android    | `barcode.content()`, `Content.parse(text, gs1)`                        | `Content` or `null`                         |

JavaScript parses during scanning, so it has no separate text call; the other
bindings parse any text you pass.

A `Content` has a `kind` and `fields` in payload order. Each field is a
`name`/`value` pair; names repeat where the payload repeats them, such as the
phone numbers of a contact. `get(name)` returns the first value and
`get_all(name)` (`getAll`, `all`) every value. Values are unescaped but
otherwise as encoded: dates, amounts and coordinates stay text, so no precision
or time-zone interpretation is added. Empty values are omitted.

Passing a barcode uses its text and GS1 flag. The GS1 flag matters: a GS1-128,
GS1 DataMatrix, GS1 QR Code or DataBar symbol has no visible marker in its
text, so element strings are parsed only when the reader reported GS1 (or you
pass `gs1 = true`). Other text is matched by its own prefix or structure.

## Kinds and fields

| Kind          | Recognized by                                                      | Fields                                                                                                                                                                                                                                                                                                                                                                   |
| ------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `GS1`         | GS1 flag; `]C1`, `]d2`, `]Q3`, `]e0`, `]J1`; `(01)…`; Digital Link | One field per element, named by its application identifier (`01`, `17`, `10`, `3103`, …). Digital Link URIs add `url`.                                                                                                                                                                                                                                                   |
| `URL`         | `http://`, `https://`                                              | `url`                                                                                                                                                                                                                                                                                                                                                                    |
| `Email`       | `mailto:`, `MATMSG:`                                               | `to` (repeated), `cc`, `bcc`, `subject`, `body`                                                                                                                                                                                                                                                                                                                          |
| `Phone`       | `tel:`                                                             | `number`                                                                                                                                                                                                                                                                                                                                                                 |
| `SMS`         | `SMSTO:`, `MMSTO:`, `sms:`                                         | `number` (repeated), `message`                                                                                                                                                                                                                                                                                                                                           |
| `Geo`         | `geo:`                                                             | `latitude`, `longitude`, `altitude`, `query`                                                                                                                                                                                                                                                                                                                             |
| `WiFi`        | `WIFI:`                                                            | `ssid`, `security` (`WPA`, `WEP`, `nopass`, …), `password`, `hidden`, `eapMethod`, `phase2Method`, `anonymousIdentity`, `identity`                                                                                                                                                                                                                                       |
| `Contact`     | vCard (`BEGIN:VCARD`), `MECARD:`                                   | `name`, `organization`, `title`, `phone`, `email`, `address`, `url`, `note`, `birthday`, `nickname` (repeated where the card repeats them)                                                                                                                                                                                                                               |
| `Calendar`    | iCalendar `VEVENT`                                                 | `summary`, `start`, `end`, `duration`, `location`, `description`, `organizer`, `url`                                                                                                                                                                                                                                                                                     |
| `SwissQRBill` | `SPC`, version 2.x, `EPD` trailer                                  | `iban`, `creditorName`, `creditorAddress`, `creditorCountry`, `amount`, `currency`, `debtorName`, `debtorAddress`, `debtorCountry`, `referenceType`, `reference`, `message`, `billingInformation`, `alternativeScheme`                                                                                                                                                   |
| `EPCPayment`  | `BCD`, version 001/002, `SCT` or `INST`                            | `bic`, `name`, `iban`, `currency`, `amount`, `purpose`, `reference`, `text`, `information`                                                                                                                                                                                                                                                                               |
| `HIBC`        | `+` and a valid mod-43 check character                             | `labelerId`, `productId`, `unitOfMeasure`; secondary data `quantity`, `expiry`, `lot` or `serial`; `link` on a separate secondary symbol                                                                                                                                                                                                                                 |
| `VIN`         | 17 characters with a valid check digit (an `I` prefix is removed)  | `vin`, `wmi`, `vds`, `vis`, `modelYear`, `plant`, `serial`                                                                                                                                                                                                                                                                                                               |
| `AAMVA`       | `@` and an `ANSI ` or `AAMVA` header                               | `issuerId`, `version`, `documentType` (`DL`, `ID`), `licenseNumber`, `lastName`, `firstName`, `middleName`, `fullName`, `dateOfBirth`, `expirationDate`, `issueDate`, `sex`, `street`, `street2`, `city`, `state`, `postalCode`, `country`, `height`, `eyeColor`, `hairColor`, `vehicleClass`, `restrictions`, `endorsements`, `documentDiscriminator`, `complianceType` |

Checks that reject a payload rather than return partial fields:

- **GS1:** unknown application identifiers, fixed-length elements of the wrong
  length or with non-digits, and GTIN/SSCC/GLN check digits.
- **Payments:** the IBAN's ISO 13616 checksum; Swiss QR-bills also need a
  Swiss or Liechtenstein IBAN, all 31 mandatory lines and the `EPD` trailer.
- **HIBC:** the mod-43 check character. **VIN:** the position-9 check digit,
  which North American VINs carry. European VINs without one are not recognized.
- **AAMVA:** a license number or last name. Element values are as encoded,
  including the jurisdiction's date order (`MMDDCCYY` in the US, `CCYYMMDD` in
  Canada).

Content can be personal data (contacts, Wi-Fi passwords, licenses). Parsing
runs locally and stores nothing; handle the returned fields like the scanned
text itself.

Not covered: Aadhaar secure QR codes (compressed and signed binary), GS1
element titles and value conversion (dates, decimal weights), Swiss QR-bill
alternative-scheme payloads, and vCard parameters such as `TYPE=WORK`.
