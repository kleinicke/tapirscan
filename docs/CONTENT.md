# Structured content

Content parsing returns the named fields of structured barcode text: GS1
element strings, Wi-Fi settings, contacts, payment slips, health-industry
labels, VINs and driver's licenses. It runs on decoded text only and never
changes what is scanned. Plain text has no content.

```python
content = tapirscan.parse_content(barcode)  # a Barcode or a str
if content and content.kind == "WiFi":
    connect(content.get("ssid"), content.get("password"))
```

| Language   | Call                                                                                                                |
| ---------- | ------------------------------------------------------------------------------------------------------------------- |
| Rust       | `barcode.content()`, `tapirscan::parse_content(text, gs1)`                                                          |
| Python     | `tapirscan.parse_content(barcode_or_text, *, gs1=None)`                                                             |
| JavaScript | `barcode.content`, filled during scanning                                                                           |
| Java       | `Tapirscan.parseContent(barcode)`, `Tapirscan.parseContent(text, gs1)`                                              |
| C++        | `tapirscan::parse_content(barcode)`, `parse_content(text, gs1)`                                                     |
| C          | `tapirscan_parse_content`, a handle (0 = no content); see the [C guide](../bindings/c/README.md#structured-content) |
| Swift      | `barcode.content`, `parseContent(text, gs1:)`                                                                       |
| Android    | `barcode.content()`, `Content.parse(text, gs1)`                                                                     |

A `Content` has a `kind` and `fields` in payload order. Each field is a
`name`/`value` pair; a name repeats when the payload repeats it, such as a
contact's phone numbers. `get(name)` returns the first value, `get_all(name)`
every value (`getAll` in Java and Android, `all` in Swift). Values are
unescaped but otherwise as encoded: dates, amounts and coordinates stay text.

Passing a barcode uses its text and GS1 flag. GS1 element strings carry no
visible marker, so they are parsed only when the reader reported GS1 or you
pass `gs1 = true`.

## Kinds

Kinds marked _experimental_ may change their fields or recognition, or be
removed, in minor releases. The other kinds and their field names are stable.

| Kind                         | Recognized by                                                      |
| ---------------------------- | ------------------------------------------------------------------ |
| `GS1`                        | GS1 flag; `]C1`, `]d2`, `]Q3`, `]e0`, `]J1`; `(01)…`; Digital Link |
| `URL`                        | `http://`, `https://`                                              |
| `Email`                      | `mailto:`, `MATMSG:`                                               |
| `Phone`                      | `tel:`                                                             |
| `SMS`                        | `SMSTO:`, `MMSTO:`, `sms:`                                         |
| `Geo`                        | `geo:`                                                             |
| `WiFi`                       | `WIFI:`                                                            |
| `Contact`                    | vCard, `MECARD:`                                                   |
| `Calendar`                   | iCalendar `VEVENT`                                                 |
| `SwissQRBill` (experimental) | `SPC` with an `EPD` trailer                                        |
| `EPCPayment` (experimental)  | SEPA credit transfer, `BCD`                                        |
| `HIBC` (experimental)        | `+` and a valid mod-43 check character                             |
| `VIN` (experimental)         | 17 characters with a valid check digit                             |
| `AAMVA` (experimental)       | `@` and an `ANSI ` or `AAMVA` header                               |

## Fields

- **GS1:** one field per element, named by its application identifier (`01`,
  `17`, `10`, `3103`, …). Digital Link URLs add `url`.
- **URL:** `url`. **Phone:** `number`.
- **Email:** `to` (repeated), `cc`, `bcc`, `subject`, `body`.
- **SMS:** `number` (repeated), `message`.
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

## Validation

A payload that fails these checks has no content, rather than partial fields:

- **GS1:** known application identifiers, fixed-length elements of the right
  length, and GTIN/SSCC/GLN check digits.
- **Payments:** the IBAN checksum; Swiss QR-bills also need a Swiss or
  Liechtenstein IBAN and all mandatory lines.
- **HIBC:** the check character. **VIN:** the position-9 check digit, so VINs
  without one (common in Europe) are not recognized.
- **AAMVA:** a license number or last name.

Not covered: Aadhaar QR codes, GS1 element titles and value conversion, Swiss
QR-bill alternative-scheme payloads, and vCard parameters such as `TYPE=WORK`.
