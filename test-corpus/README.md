# test-corpus/

Golden-file fixtures for the Contextura integration test runner.

## Structure

Each test case is a pair of files:

| File | Description |
|------|-------------|
| `<name>.png` | Screenshot input for the OCR/translation pipeline |
| `<name>.expected.json` | Assertion fixture consumed by `--test-suite` |

## Running

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- \
  --debug-cli --test-suite test-corpus
```

Exits `0` on all passes, non-zero on any failure.

## Fixture Schema

```json
{
  "description": "Human-readable label (ignored by runner)",
  "ocr_must_contain": ["fragment1", "fragment2"],
  "translation_must_contain": ["english fragment"],
  "ocr_boxes": [
    {
      "text": "日本語テキスト",
      "x": 42.0,
      "y": 100.0,
      "width": 200.0,
      "height": 32.0
    }
  ]
}
```

### Fields

- **`ocr_must_contain`** — Each string must appear as a substring in the concatenated OCR text output.
- **`translation_must_contain`** — Each string must appear (case-insensitive) in the concatenated translation output.
- **`ocr_boxes`** — Optional per-box coordinate assertions. Each expected box must find a detected OCR box whose text contains the expected `text` fragment **and** whose bounding-box coordinates (`x`, `y`, `width`, `height`) are all within **±5 pixels** of the expected values. An empty array always passes.

## Adding New Cases

1. Drop a real `.png` screenshot containing Japanese text under `test-corpus/`.
2. Run `cargo run -- --debug-cli --input test-corpus/<name>.png --pretty` to capture the OCR and translation output.
3. Create `test-corpus/<name>.expected.json` with the verified output fragments and bounding-box coordinates from step 2.
4. Re-run the test suite to confirm green.
