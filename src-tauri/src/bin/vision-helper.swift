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
        candidates
            .map { candidate in
                (candidate: candidate, score: candidateScore(candidate))
            }
            .filter { !normalize($0.candidate.string).isEmpty }
            .max { lhs, rhs in lhs.score < rhs.score }
            .map(\.candidate)
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

    private static func containsCJK(_ text: String) -> Bool {
        text.unicodeScalars.contains { scalar in
            switch scalar.value {
            case 0x3040...0x309F, 0x30A0...0x30FF, 0x4E00...0x9FFF:
                return true
            default:
                return false
            }
        }
    }

    private static func containsKana(_ text: String) -> Bool {
        text.unicodeScalars.contains { scalar in
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
