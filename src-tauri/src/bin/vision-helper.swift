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
            print("[]")
            return
        }

        let imageURL = URL(fileURLWithPath: CommandLine.arguments[1])
        guard FileManager.default.fileExists(atPath: imageURL.path) else {
            print("[]")
            return
        }

        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = ["ja-JP", "zh-Hant", "en-US"]
        request.usesLanguageCorrection = false

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
            guard let candidate = observation.topCandidates(1).first else {
                return nil
            }

            let box = observation.boundingBox
            let dx = observation.bottomRight.x - observation.bottomLeft.x
            let dy = observation.bottomRight.y - observation.bottomLeft.y
            let angle = atan2(dy, dx)

            return Result(
                text: candidate.string,
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
