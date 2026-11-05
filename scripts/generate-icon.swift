#!/usr/bin/env swift
import AppKit
import Foundation

func render(size: Int) throws -> Data {
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ), let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        throw NSError(domain: "QuickPRIcon", code: 1,
                      userInfo: [NSLocalizedDescriptionKey: "Could not create the icon canvas."])
    }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    NSGraphicsContext.current = context
    context.imageInterpolation = .high
    let transform = NSAffineTransform()
    transform.scale(by: CGFloat(size) / 1024)
    transform.concat()

    let tile = NSBezierPath(roundedRect: NSRect(x: 80, y: 80, width: 864, height: 864),
                            xRadius: 194, yRadius: 194)
    NSGradient(colors: [
        NSColor(srgbRed: 0.20, green: 0.15, blue: 0.29, alpha: 1),
        NSColor(srgbRed: 0.075, green: 0.057, blue: 0.11, alpha: 1),
    ])!.draw(in: tile, angle: -90)
    NSColor(srgbRed: 0.58, green: 0.46, blue: 0.77, alpha: 0.28).setStroke()
    tile.lineWidth = 3
    tile.stroke()

    NSColor(srgbRed: 0.74, green: 0.95, blue: 0.84, alpha: 1).setStroke()
    let branch = NSBezierPath()
    branch.lineWidth = 44
    branch.lineCapStyle = .round
    branch.move(to: NSPoint(x: 355, y: 355))
    branch.line(to: NSPoint(x: 355, y: 670))
    branch.move(to: NSPoint(x: 670, y: 355))
    branch.line(to: NSPoint(x: 670, y: 545))
    branch.curve(to: NSPoint(x: 530, y: 670), controlPoint1: NSPoint(x: 670, y: 635), controlPoint2: NSPoint(x: 620, y: 670))
    branch.line(to: NSPoint(x: 505, y: 670))
    branch.move(to: NSPoint(x: 565, y: 730))
    branch.line(to: NSPoint(x: 505, y: 670))
    branch.line(to: NSPoint(x: 565, y: 610))
    branch.stroke()
    for point in [NSPoint(x: 355, y: 730), NSPoint(x: 355, y: 295), NSPoint(x: 670, y: 295)] {
        let ring = NSBezierPath(ovalIn: NSRect(x: point.x-55, y: point.y-55, width: 110, height: 110))
        ring.lineWidth = 36
        ring.stroke()
    }

    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        throw NSError(domain: "QuickPRIcon", code: 2,
                      userInfo: [NSLocalizedDescriptionKey: "Could not encode the icon."])
    }
    return png
}

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "scripts/icons",
                 isDirectory: true)
let files = FileManager.default
try files.createDirectory(at: output, withIntermediateDirectories: true)
let temporary = files.temporaryDirectory.appendingPathComponent("quick-pr-icon-\(UUID().uuidString)")
let iconset = temporary.appendingPathComponent("QuickPR.iconset", isDirectory: true)
try files.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? files.removeItem(at: temporary) }

for side in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let suffix = scale == 2 ? "@2x" : ""
        let filename = "icon_\(side)x\(side)\(suffix).png"
        try render(size: side * scale).write(to: iconset.appendingPathComponent(filename))
    }
}
try render(size: 1024).write(to: output.appendingPathComponent("QuickPR.png"))
let iconutil = Process()
iconutil.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
iconutil.arguments = ["--convert", "icns", "--output",
                      output.appendingPathComponent("QuickPR.icns").path, iconset.path]
try iconutil.run()
iconutil.waitUntilExit()
guard iconutil.terminationStatus == 0 else {
    throw NSError(domain: "QuickPRIcon", code: Int(iconutil.terminationStatus),
                  userInfo: [NSLocalizedDescriptionKey: "iconutil could not create QuickPR.icns."])
}
print("Created \(output.appendingPathComponent("QuickPR.icns").path)")
