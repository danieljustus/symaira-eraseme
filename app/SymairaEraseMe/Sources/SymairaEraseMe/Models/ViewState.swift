import Foundation

/// Three-state model for screen content: loading, loaded (possibly empty), or failed.
enum ViewState<T> {
    case idle
    case loading
    case loaded(T)
    case failed(String)

    var errorMessage: String? {
        if case .failed(let msg) = self { return msg }
        return nil
    }

    var value: T? {
        if case .loaded(let v) = self { return v }
        return nil
    }

    var isLoaded: Bool {
        if case .loaded = self { return true }
        return false
    }

    var isLoading: Bool {
        if case .loading = self { return true }
        return false
    }
}
