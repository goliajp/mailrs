import Foundation

enum ApiError: Error, LocalizedError {
    case invalidURL
    case unauthorized
    case forbidden
    case notFound
    case rateLimited(retryAfter: TimeInterval?)
    case server(status: Int, message: String?)
    case transport(URLError)
    case decoding(DecodingError)
    case malformed(String)

    var errorDescription: String? {
        switch self {
        case .invalidURL: return "Invalid URL"
        case .unauthorized: return "登录已失效，请重新登录"
        case .forbidden: return "没有权限"
        case .notFound: return "资源不存在"
        case .rateLimited: return "请求过于频繁，请稍后再试"
        case .server(_, let msg): return msg ?? "服务器错误"
        case .transport(let e): return e.localizedDescription
        case .decoding: return "响应格式错误"
        case .malformed(let s): return s
        }
    }
}

struct ApiErrorBody: Decodable {
    let error: String?
    let message: String?
    let detail: String?

    var humanMessage: String? { message ?? error ?? detail }
}
