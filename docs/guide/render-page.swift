// Extract one page of a PDF to a PNG at a target width. macOS only (PDFKit).
// Usage: swift render-page.swift <pdf> <page-1based> <out.png> <width-px>
import AppKit
import PDFKit

let a = CommandLine.arguments
guard a.count == 5,
    let doc = PDFDocument(url: URL(fileURLWithPath: a[1])),
    let pageNum = Int(a[2]),
    let width = Double(a[4]),
    let page = doc.page(at: pageNum - 1)
else {
    FileHandle.standardError.write(
        Data("usage: render-page.swift <pdf> <page> <out.png> <width>\n".utf8))
    exit(1)
}

let r = page.bounds(for: .mediaBox)
let scale = width / Double(r.width)
let size = NSSize(width: width, height: (Double(r.height) * scale).rounded())

let img = NSImage(size: size)
img.lockFocus()
NSColor.black.set()
NSBezierPath.fill(NSRect(origin: .zero, size: size))
let ctx = NSGraphicsContext.current!.cgContext
ctx.scaleBy(x: scale, y: scale)
page.draw(with: .mediaBox, to: ctx)
img.unlockFocus()

let rep = NSBitmapImageRep(data: img.tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[3]))
