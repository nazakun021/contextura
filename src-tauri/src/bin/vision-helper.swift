import Foundation
import Vision
import AppKit

if CommandLine.arguments.count < 2 {
    print("[]")
    exit(0)
}

let imagePath = CommandLine.arguments[1]
guard let image = NSImage(contentsOfFile: imagePath),
      let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    print("[]")
    exit(0)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.recognitionLanguages = ["ja-JP"]
request.usesLanguageCorrection = true

let handler = VNImageRequestHandler(cgImage: cgImage)
try? handler.perform([request])

struct Result: Codable {
    let text: String
    let confidence: Float
    let x, y, width, height: Double
    let text_angle: Double
}

var results: [Result] = []
for obs in (request.results ?? []) {
    guard let candidate = obs.topCandidates(1).first else { continue }
    let box = obs.boundingBox
    let dx = obs.bottomRight.x - obs.bottomLeft.x
    let dy = obs.bottomRight.y - obs.bottomLeft.y
    let angle = atan2(dy, dx)
    
    results.append(Result(
        text: candidate.string,
        confidence: candidate.confidence,
        x: box.origin.x, y: box.origin.y,
        width: box.size.width, height: box.size.height,
        text_angle: angle
    ))
}

let data = try! JSONEncoder().encode(results)
print(String(data: data, encoding: .utf8)!)
