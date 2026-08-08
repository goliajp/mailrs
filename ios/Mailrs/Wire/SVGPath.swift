import CoreGraphics
import SwiftUI

/// An SVG `d` attribute, as a `Path`.
///
/// Written so the icon data can stay **verbatim** what upstream ships.
/// Lucide's paths use relative commands and elliptical arcs — `a4 4 0 0
/// 0-4-4` is an ordinary sight in them — and pre-converting those to
/// cubics in a script would leave a blob nobody can diff against the
/// source when an icon is updated. So the arc conversion lives here,
/// where it can be tested, and the icons stay copy-and-paste.
///
/// Not a general SVG renderer: no transforms, no styling, no `<use>`.
/// One attribute, all of its commands.
enum SVGPath {
    static func parse(_ d: String) -> Path {
        var path = Path()
        var tokens = Tokenizer(d)
        var current = CGPoint.zero
        var start = CGPoint.zero
        // The reflection point for smooth curves, per command family.
        var lastCubicControl: CGPoint?
        var lastQuadControl: CGPoint?
        var command: Character = " "

        while let next = tokens.nextCommand(after: command) {
            command = next
            let relative = command.isLowercase
            func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
                if relative { return CGPoint(x: current.x + x, y: current.y + y) }
                return CGPoint(x: x, y: y)
            }
            switch Character(command.lowercased()) {
            case "m":
                guard let x = tokens.number(), let y = tokens.number() else { return path }
                current = point(x, y)
                start = current
                path.move(to: current)
                lastCubicControl = nil
                lastQuadControl = nil
            case "l":
                guard let x = tokens.number(), let y = tokens.number() else { return path }
                current = point(x, y)
                path.addLine(to: current)
                lastCubicControl = nil
                lastQuadControl = nil
            case "h":
                guard let x = tokens.number() else { return path }
                if relative { current = CGPoint(x: current.x + x, y: current.y) }
                else { current = CGPoint(x: x, y: current.y) }
                path.addLine(to: current)
                lastCubicControl = nil
            case "v":
                guard let y = tokens.number() else { return path }
                if relative { current = CGPoint(x: current.x, y: current.y + y) }
                else { current = CGPoint(x: current.x, y: y) }
                path.addLine(to: current)
                lastCubicControl = nil
            case "c":
                guard let x1 = tokens.number(), let y1 = tokens.number(),
                      let x2 = tokens.number(), let y2 = tokens.number(),
                      let x = tokens.number(), let y = tokens.number() else { return path }
                let c1 = point(x1, y1), c2 = point(x2, y2)
                current = point(x, y)
                path.addCurve(to: current, control1: c1, control2: c2)
                lastCubicControl = c2
                lastQuadControl = nil
            case "s":
                guard let x2 = tokens.number(), let y2 = tokens.number(),
                      let x = tokens.number(), let y = tokens.number() else { return path }
                let c1 = reflect(lastCubicControl, about: current)
                let c2 = point(x2, y2)
                current = point(x, y)
                path.addCurve(to: current, control1: c1, control2: c2)
                lastCubicControl = c2
                lastQuadControl = nil
            case "q":
                guard let x1 = tokens.number(), let y1 = tokens.number(),
                      let x = tokens.number(), let y = tokens.number() else { return path }
                let c = point(x1, y1)
                current = point(x, y)
                path.addQuadCurve(to: current, control: c)
                lastQuadControl = c
                lastCubicControl = nil
            case "t":
                guard let x = tokens.number(), let y = tokens.number() else { return path }
                let c = reflect(lastQuadControl, about: current)
                current = point(x, y)
                path.addQuadCurve(to: current, control: c)
                lastQuadControl = c
                lastCubicControl = nil
            case "a":
                guard let rx = tokens.number(), let ry = tokens.number(),
                      let rotation = tokens.number(), let large = tokens.flag(),
                      let sweep = tokens.flag(),
                      let x = tokens.number(), let y = tokens.number() else { return path }
                let end = point(x, y)
                addArc(to: &path, from: current, to: end, rx: rx, ry: ry,
                       rotation: rotation, largeArc: large, sweep: sweep)
                current = end
                lastCubicControl = nil
                lastQuadControl = nil
            case "z":
                path.closeSubpath()
                current = start
                lastCubicControl = nil
                lastQuadControl = nil
            default:
                return path
            }
        }
        return path
    }

    private static func reflect(_ control: CGPoint?, about point: CGPoint) -> CGPoint {
        // With no previous curve the control point *is* the current
        // point, which the spec says explicitly.
        guard let control else { return point }
        return CGPoint(x: 2 * point.x - control.x, y: 2 * point.y - control.y)
    }

    /// Endpoint arcs, converted to centre parameterisation and drawn as
    /// one or more cubics — SVG 1.1 §F.6.5, the standard construction.
    private static func addArc(
        to path: inout Path, from p0: CGPoint, to p1: CGPoint,
        rx rxIn: CGFloat, ry ryIn: CGFloat, rotation: CGFloat,
        largeArc: Bool, sweep: Bool
    ) {
        // Degenerate radii mean a straight line, not an error.
        var rx = abs(rxIn), ry = abs(ryIn)
        if rx == 0 || ry == 0 || (p0.x == p1.x && p0.y == p1.y) {
            path.addLine(to: p1)
            return
        }
        let phi = rotation * .pi / 180
        let cosPhi = cos(phi), sinPhi = sin(phi)
        let dx = (p0.x - p1.x) / 2, dy = (p0.y - p1.y) / 2
        let x1 = cosPhi * dx + sinPhi * dy
        let y1 = -sinPhi * dx + cosPhi * dy

        // Radii too small to reach: scaled up, again per the spec, which
        // is what keeps a rounded rectangle from tearing.
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lambda > 1 {
            rx *= sqrt(lambda)
            ry *= sqrt(lambda)
        }

        let sign: CGFloat = (largeArc != sweep) ? 1 : -1
        let numerator = max(0, rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1)
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1
        let coefficient = sign * sqrt(numerator / max(denominator, .leastNonzeroMagnitude))
        let cx1 = coefficient * rx * y1 / ry
        let cy1 = -coefficient * ry * x1 / rx
        let cx = cosPhi * cx1 - sinPhi * cy1 + (p0.x + p1.x) / 2
        let cy = sinPhi * cx1 + cosPhi * cy1 + (p0.y + p1.y) / 2

        func angle(_ ux: CGFloat, _ uy: CGFloat, _ vx: CGFloat, _ vy: CGFloat) -> CGFloat {
            let dot = ux * vx + uy * vy
            let len = sqrt((ux * ux + uy * uy) * (vx * vx + vy * vy))
            var value = acos(min(1, max(-1, dot / max(len, .leastNonzeroMagnitude))))
            if ux * vy - uy * vx < 0 { value = -value }
            return value
        }
        let theta = angle(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry)
        var delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry)
        if !sweep, delta > 0 { delta -= 2 * .pi }
        if sweep, delta < 0 { delta += 2 * .pi }

        // One cubic per quarter turn or less: the error of the standard
        // approximation grows with the sweep, and a quarter is where it
        // stops being visible.
        let segments = max(1, Int(ceil(abs(delta) / (.pi / 2))))
        let step = delta / CGFloat(segments)
        let alpha = 4.0 / 3.0 * tan(step / 4)
        var angleStart = theta
        for _ in 0..<segments {
            let angleEnd = angleStart + step
            let (cosA, sinA) = (cos(angleStart), sin(angleStart))
            let (cosB, sinB) = (cos(angleEnd), sin(angleEnd))
            func onEllipse(_ c: CGFloat, _ s: CGFloat) -> CGPoint {
                CGPoint(x: cx + rx * cosPhi * c - ry * sinPhi * s,
                        y: cy + rx * sinPhi * c + ry * cosPhi * s)
            }
            func derivative(_ c: CGFloat, _ s: CGFloat) -> CGPoint {
                CGPoint(x: -rx * cosPhi * s - ry * sinPhi * c,
                        y: -rx * sinPhi * s + ry * cosPhi * c)
            }
            let from = onEllipse(cosA, sinA), to = onEllipse(cosB, sinB)
            let d1 = derivative(cosA, sinA), d2 = derivative(cosB, sinB)
            path.addCurve(
                to: to,
                control1: CGPoint(x: from.x + alpha * d1.x, y: from.y + alpha * d1.y),
                control2: CGPoint(x: to.x - alpha * d2.x, y: to.y - alpha * d2.y)
            )
            angleStart = angleEnd
        }
    }

    /// SVG's number syntax, which is not Swift's: `1.5.5` is two
    /// numbers, `-4-4` is two numbers, and a command letter ends one.
    private struct Tokenizer {
        private let characters: [Character]
        private var index = 0

        init(_ d: String) {
            characters = Array(d)
        }

        private mutating func skipSeparators() {
            while index < characters.count,
                  characters[index] == " " || characters[index] == ","
                    || characters[index] == "\n" || characters[index] == "\t" {
                index += 1
            }
        }

        /// The next command letter, or a repeat of the previous one when
        /// the argument list simply continues — `M0 0 1 1 2 2` is a move
        /// and two lines.
        mutating func nextCommand(after previous: Character) -> Character? {
            skipSeparators()
            guard index < characters.count else { return nil }
            if characters[index].isLetter {
                let command = characters[index]
                index += 1
                return command
            }
            guard previous != " " else { return nil }
            // A repeated `m` continues as `l`, per the spec.
            if previous == "m" { return "l" }
            if previous == "M" { return "L" }
            return previous
        }

        mutating func number() -> CGFloat? {
            skipSeparators()
            var text = ""
            if index < characters.count, characters[index] == "-" || characters[index] == "+" {
                text.append(characters[index])
                index += 1
            }
            var seenDot = false
            while index < characters.count {
                let character = characters[index]
                if character.isNumber {
                    text.append(character)
                    index += 1
                } else if character == "." && !seenDot {
                    seenDot = true
                    text.append(character)
                    index += 1
                } else if character == "e" || character == "E" {
                    text.append(character)
                    index += 1
                    if index < characters.count,
                       characters[index] == "-" || characters[index] == "+" {
                        text.append(characters[index])
                        index += 1
                    }
                } else {
                    break
                }
            }
            guard let value = Double(text) else { return nil }
            return CGFloat(value)
        }

        /// Arc flags are single characters and may be written without
        /// separators: `0 0 1` and `001` are the same three flags.
        mutating func flag() -> Bool? {
            skipSeparators()
            guard index < characters.count else { return nil }
            let character = characters[index]
            guard character == "0" || character == "1" else { return number().map { $0 != 0 } }
            index += 1
            return character == "1"
        }
    }
}
