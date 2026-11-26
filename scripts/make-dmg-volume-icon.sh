#!/usr/bin/env bash
set -euo pipefail

# Composites the app icon onto macOS's native removable disk drive icon and
# writes a Retina-ready .icns file suitable for a DMG's .VolumeIcon.icns.

APP_ICON="${1:-assets/icon.png}"
OUTPUT_ICNS="${2:?usage: make-dmg-volume-icon.sh <app-icon.png> <output.icns>}"
BASE_DISK_ICNS="${DMG_BASE_DISK_ICNS:-/System/Library/Extensions/IOStorageFamily.kext/Contents/Resources/Removable.icns}"

if [ ! -f "$APP_ICON" ]; then
  echo "error: app icon not found at $APP_ICON" >&2
  exit 1
fi

if [ ! -f "$BASE_DISK_ICNS" ]; then
  echo "error: macOS base disk icon not found at $BASE_DISK_ICNS" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bruk-dmg-icon.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

SWIFT_CACHE_DIR="${TMP_DIR}/swift-module-cache"
mkdir -p "$SWIFT_CACHE_DIR" "$(dirname "$OUTPUT_ICNS")"

swift -module-cache-path "$SWIFT_CACHE_DIR" - "$BASE_DISK_ICNS" "$APP_ICON" "$OUTPUT_ICNS" <<'SWIFT'
import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let args = CommandLine.arguments
guard args.count == 4 else {
    fputs("error: expected <base-disk-icns> <app-icon> <output-icns>\n", stderr)
    exit(1)
}

let baseURL = URL(fileURLWithPath: args[1])
let appIconURL = URL(fileURLWithPath: args[2])
let outputURL = URL(fileURLWithPath: args[3])

guard let baseImage = NSImage(contentsOf: baseURL),
      let appImage = NSImage(contentsOf: appIconURL),
      let srgb = CGColorSpace(name: CGColorSpace.sRGB) else {
    fputs("error: failed to load base disk icon or app icon\n", stderr)
    exit(1)
}

let canvasSize = 1024
var baseRect = NSRect(x: 0, y: 0, width: canvasSize, height: canvasSize)
guard let baseCG = baseImage.cgImage(forProposedRect: &baseRect, context: nil, hints: nil),
      let appCG = appImage.cgImage(forProposedRect: &baseRect, context: nil, hints: nil) else {
    fputs("error: failed to decode CGImage\n", stderr)
    exit(1)
}

let bytesPerPixel = 4
let bytesPerRow = canvasSize * bytesPerPixel
var pixels = [UInt8](repeating: 0, count: canvasSize * bytesPerRow)

guard let ctx = CGContext(
    data: &pixels,
    width: canvasSize,
    height: canvasSize,
    bitsPerComponent: 8,
    bytesPerRow: bytesPerRow,
    space: srgb,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
) else {
    fputs("error: failed to create CGContext\n", stderr)
    exit(1)
}

ctx.interpolationQuality = .high
ctx.draw(baseCG, in: CGRect(x: 0, y: 0, width: canvasSize, height: canvasSize))

// Seamlessly remove the built-in downward arrow on Removable.icns by linearly
// interpolating between untouched drive surface rows above (y=256) and below
// (y=668) the arrow across the flat center of the drive face (top-down coords).
let topRefY = 256
let botRefY = 668
let patchMinX = 250
let patchMaxX = 774

for x in patchMinX...patchMaxX {
    let topOffset = topRefY * bytesPerRow + x * bytesPerPixel
    let botOffset = botRefY * bytesPerRow + x * bytesPerPixel
    for y in (topRefY + 1)..<botRefY {
        let t = Double(y - topRefY) / Double(botRefY - topRefY)
        let dst = y * bytesPerRow + x * bytesPerPixel
        for c in 0..<4 {
            let v0 = Double(pixels[topOffset + c])
            let v1 = Double(pixels[botOffset + c])
            pixels[dst + c] = UInt8((v0 * (1.0 - t) + v1 * t).rounded())
        }
    }
}

// Composite the app icon centered on the drive's top face (y=80..836 top-down -> center y=458).
// In CoreGraphics bottom-up coordinates, centerY = 1024 - 458 = 566.
let badgeSize: CGFloat = 530
let badgeRect = CGRect(
    x: (CGFloat(canvasSize) - badgeSize) / 2.0,
    y: 566.0 - badgeSize / 2.0,
    width: badgeSize,
    height: badgeSize
)

ctx.saveGState()
ctx.setShadow(
    offset: CGSize(width: 0, height: -10),
    blur: 18,
    color: CGColor(colorSpace: srgb, components: [0, 0, 0, 0.22])
)
ctx.draw(appCG, in: badgeRect)
ctx.restoreGState()

guard let compositedCG = ctx.makeImage() else {
    fputs("error: failed to finalize composited image\n", stderr)
    exit(1)
}

func renderPNG(size px: Int) -> Data {
    guard let scaledCtx = CGContext(
        data: nil,
        width: px,
        height: px,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: srgb,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        fputs("error: failed to create \(px)x\(px) CGContext\n", stderr)
        exit(1)
    }
    scaledCtx.interpolationQuality = .high
    scaledCtx.draw(compositedCG, in: CGRect(x: 0, y: 0, width: px, height: px))
    guard let scaledImage = scaledCtx.makeImage() else {
        fputs("error: failed to render \(px)x\(px) image\n", stderr)
        exit(1)
    }
    let pngData = NSMutableData()
    guard let dest = CGImageDestinationCreateWithData(
        pngData,
        UTType.png.identifier as CFString,
        1,
        nil
    ) else {
        fputs("error: failed to create PNG destination for \(px)x\(px)\n", stderr)
        exit(1)
    }
    CGImageDestinationAddImage(dest, scaledImage, nil)
    guard CGImageDestinationFinalize(dest) else {
        fputs("error: failed to finalize PNG for \(px)x\(px)\n", stderr)
        exit(1)
    }
    return pngData as Data
}

// Standard macOS PNG-in-ICNS OSType chunks (16x16 through 512x512@2x).
let chunks: [(String, Int)] = [
    ("icp4", 16),
    ("ic11", 32),
    ("icp5", 32),
    ("ic12", 64),
    ("ic07", 128),
    ("ic13", 256),
    ("ic08", 256),
    ("ic14", 512),
    ("ic09", 512),
    ("ic10", 1024),
]

var pngCache: [Int: Data] = [:]
var body = Data()

for (ostype, px) in chunks {
    let png = pngCache[px] ?? {
        let rendered = renderPNG(size: px)
        pngCache[px] = rendered
        return rendered
    }()
    body.append(contentsOf: ostype.utf8)
    var chunkSize = UInt32(8 + png.count).bigEndian
    withUnsafeBytes(of: &chunkSize) { body.append(contentsOf: $0) }
    body.append(png)
}

var icns = Data("icns".utf8)
var totalSize = UInt32(8 + body.count).bigEndian
withUnsafeBytes(of: &totalSize) { icns.append(contentsOf: $0) }
icns.append(body)

try icns.write(to: outputURL, options: .atomic)
SWIFT

echo "Generated DMG volume icon at $OUTPUT_ICNS"
