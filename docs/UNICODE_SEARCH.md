# Unicode search

History title, creator, description and transcript filters, OCR pages and legacy
OCR text, subtitle/transcript/description readers and keyword filters use the same
literal, locale-independent substring comparison. The comparison key is canonical
NFD, full default Unicode case folding, then canonical NFC. Accents are preserved;
this is not accent stripping or compatibility normalization.

Examples: `ÖSTERREICH` matches `Österreich` and `O` + combining diaeresis;
`STRASSE` matches `Straße`; Greek final sigma and ordinary sigma match. `cafe` does
not match `café`. `%`, `_`, backslashes and regex metacharacters stay literal.
Leading and trailing Unicode whitespace and BOM are trimmed from the query.

The pinned [Unicode 17 folding data](https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt)
is checked in under `vendor/unicode`, with its Unicode License V3. Only C and F
mappings are used, following the file's full default folding rules; locale-specific
Turkic overrides are excluded. The frontend artifact includes the license notice.
Rust uses the already-locked ICU normalizer; browsers use `String.normalize`.

SQLite registers `pinefetch_contains` on every initialized connection and caches
the folded query with SQLite auxiliary data. NULL titles/creators do not match.
Search keys also sign cursor filters, allowing equivalent case/normalization
variants to continue the same ordered results. Raw creator identity and indexed
exact Account navigation stay distinct from text search. No schema migration or
rewrite of saved content is required.

Text highlights map folded offsets back to complete original grapheme clusters.
This handles both length expansion (`ß` to `ss`) and combining sequences without
changing display/copy content or timestamp navigation.

Regenerate the frontend table after an intentional Unicode data update:

```sh
node scripts/generate-search-folding.mjs --write
node scripts/generate-search-folding.mjs --check
```

JavaScript and native SQLite tests consume the same Unicode corpus and validate
all pinned folding mappings. Chromium/WebKit tests run that corpus and the real
text reader under a Turkish browser locale, including highlights, copying and
seeking. Large-archive regressions retain exact-creator index behavior and verify
Unicode cursor results on 200,000 synthetic rows. These checks use isolated data.
