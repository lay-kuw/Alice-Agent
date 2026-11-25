import Foundation

/// Google Calendar's eleven event color tags, as softened pastel tones.
public enum EventColor {
    public struct Tag: Identifiable, Sendable {
        public let id: String
        public let name: String
        /// sRGB 0...1
        public let rgb: (Double, Double, Double)
    }

    public static let tags: [Tag] = [
        Tag(id: "1", name: "Lavender", rgb: (0.64, 0.68, 0.93)),
        Tag(id: "2", name: "Sage", rgb: (0.55, 0.84, 0.70)),
        Tag(id: "3", name: "Grape", rgb: (0.78, 0.60, 0.92)),
        Tag(id: "4", name: "Flamingo", rgb: (0.97, 0.63, 0.66)),
        Tag(id: "5", name: "Banana", rgb: (0.98, 0.84, 0.45)),
        Tag(id: "6", name: "Tangerine", rgb: (1.0, 0.69, 0.52)),
        Tag(id: "7", name: "Peacock", rgb: (0.47, 0.77, 0.96)),
        Tag(id: "8", name: "Graphite", rgb: (0.70, 0.70, 0.74)),
        Tag(id: "9", name: "Blueberry", rgb: (0.52, 0.60, 0.91)),
        Tag(id: "10", name: "Basil", rgb: (0.42, 0.75, 0.58)),
        Tag(id: "11", name: "Tomato", rgb: (0.94, 0.47, 0.52)),
    ]

    public static func tag(_ id: String?) -> Tag? { tags.first { $0.id == id } }
}
