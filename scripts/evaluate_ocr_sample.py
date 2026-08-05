#!/usr/bin/env python3
"""Calculate normalized page-level CER for Contextura OCR benchmark samples."""

import argparse
import json
import unicodedata
from pathlib import Path


def normalize(text: str) -> str:
    return "".join(unicodedata.normalize("NFKC", text).split())


def levenshtein_distance(prediction: str, reference: str) -> int:
    previous = list(range(len(reference) + 1))
    for predicted_character in prediction:
        current = [previous[0] + 1]
        for index, reference_character in enumerate(reference, start=1):
            current.append(
                min(
                    previous[index] + 1,
                    current[index - 1] + 1,
                    previous[index - 1] + (predicted_character != reference_character),
                )
            )
        previous = current
    return previous[-1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("ground_truth", type=Path)
    parser.add_argument("predictions_dir", type=Path)
    args = parser.parse_args()

    ground_truth = json.loads(args.ground_truth.read_text())
    total_distance = 0
    total_reference_characters = 0
    pages = []

    for item in ground_truth:
        prediction_path = args.predictions_dir / Path(item["file"]).with_suffix(".ocr.json")
        prediction = json.loads(prediction_path.read_text())
        reference_text = normalize(item["text"])
        predicted_text = normalize("".join(prediction["ocr"]))
        distance = levenshtein_distance(predicted_text, reference_text)
        page = {
            "file": item["file"],
            "reference_characters": len(reference_text),
            "predicted_characters": len(predicted_text),
            "edit_distance": distance,
            "cer": distance / len(reference_text) if reference_text else 0.0,
        }
        pages.append(page)
        total_distance += distance
        total_reference_characters += len(reference_text)

    report = {
        "normalization": "Unicode NFKC followed by whitespace removal",
        "pages": pages,
        "micro_cer": total_distance / total_reference_characters,
        "total_edit_distance": total_distance,
        "total_reference_characters": total_reference_characters,
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()