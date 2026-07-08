// src-tauri/src/bin/vision-helper.swift

import Foundation
import AppKit
import Vision

struct Result: Codable {
    let text: String
    let confidence: Float
    let x: Double
    let y: Double
    let width: Double
    let height: Double
    let text_angle: Double
}

@main
struct VisionHelper {
    static func main() {
        autoreleasepool {
            run()
        }
    }

    private static func run() {
        // Vision text recognition is unreliable in this standalone helper unless Cocoa is initialized first.
        _ = NSApplication.shared

        guard CommandLine.arguments.count >= 2 else {
            fail("Missing image path argument", code: 64)
        }

        let imageURL = URL(fileURLWithPath: CommandLine.arguments[1])
        guard FileManager.default.fileExists(atPath: imageURL.path) else {
            fail("Input image does not exist at \(imageURL.path)", code: 66)
        }
        guard fileSize(at: imageURL) > 0 else {
            fail("Input image is empty at \(imageURL.path)", code: 65)
        }

        let request = configuredRequest()

        do {
            try perform(request: request, imageURL: imageURL)
        } catch let VisionHelperError.requestFailed(urlError, cgImageError) {
            fputs(
                "vision-helper: Vision request failed. url_handler=\(urlError); cgimage_handler=\(cgImageError)\n",
                stderr
            )
            exit(1)
        } catch {
            fputs("vision-helper: Vision request failed: \(error)\n", stderr)
            exit(1)
        }

        let results = (request.results ?? []).compactMap { observation -> Result? in
            guard let candidate = selectBestCandidate(from: observation.topCandidates(3)) else {
                return nil
            }

            let text = normalize(candidate.string)
            guard !text.isEmpty else {
                return nil
            }

            let box = observation.boundingBox
            let dx = observation.bottomRight.x - observation.bottomLeft.x
            let dy = observation.bottomRight.y - observation.bottomLeft.y
            let angle = atan2(dy, dx)

            return Result(
                text: text,
                confidence: candidate.confidence,
                x: box.origin.x,
                y: box.origin.y,
                width: box.size.width,
                height: box.size.height,
                text_angle: angle
            )
        }

        do {
            let data = try JSONEncoder().encode(results)
            guard let output = String(data: data, encoding: .utf8) else {
                fputs("vision-helper: Failed to encode JSON output\n", stderr)
                exit(1)
            }
            print(output)
        } catch {
            fputs("vision-helper: Failed to encode OCR results: \(error)\n", stderr)
            exit(1)
        }
    }

    private static func configuredRequest() -> VNRecognizeTextRequest {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["ja-JP", "zh-Hant", "en-US"]
        request.usesLanguageCorrection = false
        request.minimumTextHeight = 0.012
        return request
    }

    private static func perform(
        request: VNRecognizeTextRequest,
        imageURL: URL
    ) throws {
        do {
            try VNImageRequestHandler(url: imageURL).perform([request])
            return
        } catch {
            let urlError = error

            do {
                guard let image = NSImage(contentsOf: imageURL),
                      let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                    throw VisionHelperError.cgImageDecodeFailed(imageURL.path)
                }
                try VNImageRequestHandler(cgImage: cgImage).perform([request])
                return
            } catch {
                throw VisionHelperError.requestFailed(urlError: urlError, cgImageError: error)
            }
        }
    }

    private static func selectBestCandidate(
        from candidates: [VNRecognizedText]
    ) -> VNRecognizedText? {
        let scored = candidates
            .map { ($0, normalize($0.string)) }
            .filter { !$1.isEmpty }

        guard let topByConfidence = scored.max(by: { $0.0.confidence < $1.0.confidence }) else {
            return nil
        }

        // Only prefer a CJK candidate if it's within a small confidence margin
        // of the top candidate — otherwise a low-confidence CJK misread can't
        // outrank a clearly correct English (or other) reading.
        let margin: Float = 0.15
        let bestCJK = scored
            .filter { containsCJK($1) && $0.confidence >= topByConfidence.0.confidence - margin }
            .max { a, b in candidateScore(a.0) < candidateScore(b.0) }

        return (bestCJK ?? scored.max { candidateScore($0.0) < candidateScore($1.0) })?.0
    }

    private static func candidateScore(_ candidate: VNRecognizedText) -> Float {
        let normalized = normalize(candidate.string)
        var score = candidate.confidence

        if containsCJK(normalized) {
            score += 0.35
        }
        if containsKana(normalized) {
            score += 0.1
        }

        return score
    }

    private static func normalize(_ text: String) -> String {
        text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }

    private static func isLikelyMisreadDash(_ text: String) -> Bool {
        // U+30FC is visually near-identical to a hyphen/dash and commonly
        // misrecognized in place of one, especially adjacent to digits/punctuation
        // (timestamps, progress bars, ranges).
        guard text.unicodeScalars.contains(where: { $0.value == 0x30FC }) else { return false }
        let strippedOfMark = text.unicodeScalars.filter { $0.value != 0x30FC }
        let hasDigitsOrAscii = strippedOfMark.contains { 
            CharacterSet.alphanumerics.contains($0) && $0.isASCII 
        }
        let hasRealJapanese = text.unicodeScalars.contains {
            (0x3040...0x309F).contains($0.value) || (0x4E00...0x9FFF).contains($0.value)
        }
        return hasDigitsOrAscii && !hasRealJapanese
    }

    private static func containsCJK(_ text: String) -> Bool {
        if isLikelyMisreadDash(text) {
            return false
        }
        return text.unicodeScalars.contains { scalar in
            switch scalar.value {
            case 0x3040...0x309F, 0x30A0...0x30FF, 0x4E00...0x9FFF:
                return true
            default:
                return false
            }
        }
    }

    private static func containsKana(_ text: String) -> Bool {
        if isLikelyMisreadDash(text) {
            return false
        }
        return text.unicodeScalars.contains { scalar in
            switch scalar.value {
            case 0x3040...0x309F, 0x30A0...0x30FF:
                return true
            default:
                return false
            }
        }
    }

    private static func fileSize(at url: URL) -> UInt64 {
        ((try? FileManager.default.attributesOfItem(atPath: url.path)[.size]) as? NSNumber)?
            .uint64Value ?? 0
    }

    private static func fail(_ message: String, code: Int32) -> Never {
        fputs("vision-helper: \(message)\n", stderr)
        exit(code)
    }
}

private enum VisionHelperError: Error, CustomStringConvertible {
    case cgImageDecodeFailed(String)
    case requestFailed(urlError: Error, cgImageError: Error)

    var description: String {
        switch self {
        case let .cgImageDecodeFailed(path):
            return "Failed to decode CGImage from \(path)"
        case let .requestFailed(urlError, cgImageError):
            return "url_handler=\(urlError); cgimage_handler=\(cgImageError)"
        }
    }
}
