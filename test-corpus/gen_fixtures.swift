#!/usr/bin/env swift
// Generates test-corpus PNG fixtures with real Japanese text using CoreGraphics.
// Usage: swift test-corpus/gen_fixtures.swift

import Foundation
import AppKit

struct TextItem {
    let text: String
    let x: CGFloat
    let y: CGFloat
    let fontSize: CGFloat
    let foreground: NSColor
}

struct Fixture {
    let filename: String
    let width: Int
    let height: Int
    let background: NSColor
    let textItems: [TextItem]
    let description: String
}

let fixtures: [Fixture] = [
    Fixture(
        filename: "case1-dialog",
        width: 640, height: 480,
        background: NSColor(calibratedRed: 0.08, green: 0.06, blue: 0.12, alpha: 1.0),
        textItems: [
            TextItem(text: "勇者よ、魔王を倒せ！", x: 80, y: 180, fontSize: 32, foreground: .white),
            TextItem(text: "はい", x: 200, y: 300, fontSize: 28, foreground: NSColor(calibratedRed: 0.9, green: 0.8, blue: 0.2, alpha: 1.0)),
            TextItem(text: "いいえ", x: 340, y: 300, fontSize: 28, foreground: NSColor(calibratedRed: 0.7, green: 0.7, blue: 0.7, alpha: 1.0)),
        ],
        description: "Game dialog — dark background, Japanese RPG text with choices"
    ),
    Fixture(
        filename: "case2-subtitle",
        width: 1280, height: 720,
        background: NSColor(calibratedRed: 0.05, green: 0.05, blue: 0.1, alpha: 1.0),
        textItems: [
            TextItem(text: "この映画は面白いですね。", x: 340, y: 640, fontSize: 36, foreground: .white),
        ],
        description: "Subtitle bar — wide dark background, sentence subtitle"
    ),
    Fixture(
        filename: "case3-webpage",
        width: 800, height: 600,
        background: .white,
        textItems: [
            TextItem(text: "日本語のウェブページ", x: 100, y: 60, fontSize: 30, foreground: .black),
            TextItem(text: "東京は日本の首都です。", x: 100, y: 140, fontSize: 22, foreground: NSColor(calibratedRed: 0.2, green: 0.2, blue: 0.2, alpha: 1.0)),
            TextItem(text: "大阪・京都・横浜も大きな都市です。", x: 100, y: 200, fontSize: 22, foreground: NSColor(calibratedRed: 0.2, green: 0.2, blue: 0.2, alpha: 1.0)),
        ],
        description: "Web page — white background, Japanese article excerpt"
    ),
]

func renderFixture(_ fixture: Fixture, outputDir: URL) throws {
    let w = fixture.width
    let h = fixture.height

    // Create image
    let image = NSImage(size: NSSize(width: w, height: h))
    image.lockFocus()

    // Fill background
    fixture.background.setFill()
    NSRect(x: 0, y: 0, width: w, height: h).fill()

    let paragraphStyle = NSMutableParagraphStyle()
    paragraphStyle.alignment = .left

    for item in fixture.textItems {
        let font = NSFont(name: "HiraginoSans-W3", size: item.fontSize)
            ?? NSFont(name: "HiraKakuProN-W3", size: item.fontSize)
            ?? NSFont.systemFont(ofSize: item.fontSize)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: item.foreground,
            .paragraphStyle: paragraphStyle,
        ]
        // NSImage coordinate system: y=0 is bottom-left
        let rect = NSRect(
            x: item.x,
            y: CGFloat(h) - item.y - item.fontSize * 1.6,
            width: CGFloat(w) - item.x - 20,
            height: item.fontSize * 2
        )
        item.text.draw(in: rect, withAttributes: attrs)
    }

    image.unlockFocus()

    // Get TIFF representation then convert to PNG
    guard let tiffData = image.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiffData),
          let pngData = bitmap.representation(using: .png, properties: [:]) else {
        throw NSError(domain: "gen_fixtures", code: 1,
            userInfo: [NSLocalizedDescriptionKey: "Failed to encode PNG for \(fixture.filename)"])
    }

    let pngURL = outputDir.appendingPathComponent("\(fixture.filename).png")
    try pngData.write(to: pngURL)
    let size = pngData.count
    print("✓ Wrote \(fixture.filename).png (\(fixture.width)×\(fixture.height), \(size / 1024) KB)")
}

// Determine output directory
let scriptURL = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
let outputDir: URL
if scriptURL.lastPathComponent == "test-corpus" {
    outputDir = scriptURL
} else {
    outputDir = scriptURL.appendingPathComponent("test-corpus")
}

do {
    for fixture in fixtures {
        try renderFixture(fixture, outputDir: outputDir)
    }
    print("\nAll fixtures generated.")
    print("Tip: run the debug-cli on each PNG to capture real OCR output,")
    print("then update the expected.json files with actual bounding boxes.")
} catch {
    fputs("Error: \(error)\n", stderr)
    exit(1)
}
