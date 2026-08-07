import Foundation

/// Plain decimal rendering for integer IDs and counts.
///
/// SwiftUI's `Text("\(int)")` interpolation applies locale-dependent
/// thousands grouping (e.g. "1.273" on de_DE), which corrupts request IDs
/// and counts. `plainDigits` renders locale-free digits, matching the
/// existing `Text("PID \(String(pid))")` pattern (see #641).
extension Int {
    /// The receiver as plain decimal digits without any grouping separators
    /// (e.g. `3221`, never `3.221`).
    var plainDigits: String { String(self) }
}
