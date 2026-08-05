# OCR Benchmarks

## MultiFinBen JapaneseOCR Sample

**Date:** 2026-08-05  
**Contextura revision:** `2417205`  
**Dataset:** [TheFinAI/MultiFinBen-JapaneseOCR](https://huggingface.co/datasets/TheFinAI/MultiFinBen-JapaneseOCR)  
**License:** Apache-2.0

### Scope

MultiFinBen JapaneseOCR contains 17,586 page images from Japanese Financial
Services Agency documents paired with page text. It is an image-to-text
benchmark, not a screen-translation benchmark, so this result measures document
OCR behavior rather than the game, manga, and UI text Contextura primarily
targets.

The full corpus is 13.4 GB. Contextura was evaluated on the public rows API's
fixed first ten training rows (`offset=0`, `length=10`) without storing the
downloaded pages in this repository.

### Method

1. Fetch the fixed sample from the Hugging Face dataset server.
2. Run each PNG through Contextura with `--debug-cli --ocr-only`, which uses the
   production `OcrEngine` and post-processing but does not start `llama-server`.
3. Concatenate returned OCR strings in Contextura reading order.
4. Normalize prediction and reference using Unicode NFKC, then remove all
   whitespace.
5. Calculate page-level Levenshtein character error rate (CER) and micro-average
   the edit distances over all reference characters.

### Result

| Pages | Reference characters | Edit distance | Micro CER |
| ----- | -------------------: | ------------: | --------: |
| 10    |                8,008 |         5,228 |    65.28% |

The page-level CER range was 4.06% to 92.01%. This wide range is expected for
page images with mixed layouts. The dataset card notes that the reference text
is extracted with PyMuPDF, so document extraction order can differ from
Contextura's spatial reading order and inflate page-level CER. This sample is
not comparable to published leaderboard results and should not be represented as
a full-corpus score.

### Reproduction

Use `scripts/evaluate_ocr_sample.py` to calculate normalized CER from a JSON
ground-truth list and matching `*.ocr.json` files emitted by Contextura:

```bash
/usr/local/bin/python3 scripts/evaluate_ocr_sample.py \
  /path/to/ground-truth.json \
  /path/to/predictions
```

For access-controlled manga text evaluation, Manga109 is a suitable future
benchmark: its annotations include Japanese text and bounding boxes, but image
access requires an application. See the [Manga109 API](https://github.com/manga109/manga109api).

## Japanese Synthetic OCR 150k Sample

**Date:** 2026-08-05  
**Contextura revision:** `2417205`  
**Dataset:** [deepcopy/japanese-synthetic-ocr-150k](https://huggingface.co/datasets/deepcopy/japanese-synthetic-ocr-150k)

### Scope

This public corpus has 150,000 synthetic Japanese text images. Contextura was
evaluated on the fixed first ten training rows (`offset=0`, `length=10`). The
rows contained only 2 to 16 reference characters and were delivered as JPEGs.

### Result

| Pages | Reference characters | Edit distance | Micro CER |
| ----- | -------------------: | ------------: | --------: |
| 10    |                   68 |            63 |    92.65% |

Eight of ten images produced no OCR strings. This is primarily a compatibility
finding, not a raw glyph-recognition comparison: Contextura's production post
processor deliberately filters small and low-context Japanese detections,
including many single-character kana strings. That policy is useful for reducing
overlay noise but makes this short-string synthetic corpus a poor proxy for
Contextura's intended screen-text workload.
