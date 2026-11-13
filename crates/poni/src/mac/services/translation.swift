import Foundation
import NaturalLanguage
import Translation

private typealias TranslationCallback = @convention(c) (
    UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?
) -> Void

private enum MailTranslationError: Error {
    case missingModels
    case unsupportedLanguage
    case unidentifiedLanguage
}

/// 请求句柄由 Rust 持有；异步任务另持有一次引用，取消和完成共用同一出口。
@MainActor
private final class MailTranslationRequest {
    private let subject: String
    private let body: String
    // FFI 值可在入口线程赋值，之后只在主 actor 的唯一完成回调中读取。
    nonisolated(unsafe) private let callback: TranslationCallback
    nonisolated(unsafe) private let context: UnsafeMutableRawPointer?
    private var task: Task<Void, Never>?
    private var cancelSession: (() -> Void)?
    private var finished = false

    nonisolated init(subject: String, body: String, callback: @escaping TranslationCallback, context: UnsafeMutableRawPointer?) {
        self.subject = subject
        self.body = body
        self.callback = callback
        self.context = context
    }

    func start() {
        guard !finished else { return }
        guard #available(macOS 26.0, *) else {
            finish(status: 1, error: "本机邮件翻译需要 macOS 26 或更新版本")
            return
        }
        task = Task { await translate() }
    }

    func cancel() {
        guard !finished else { return }
        task?.cancel()
        cancelSession?()
        finish(status: 2, error: "翻译已取消")
    }

    @available(macOS 26.0, *)
    private func translate() async {
        guard !finished else { return }
        do {
            // 标题和正文可以是不同语言；无法识别短标题时才借用正文的语种。
            let subjectLanguage = NLLanguageRecognizer.dominantLanguage(for: subject)
            let bodyLanguage = NLLanguageRecognizer.dominantLanguage(for: body)
            let translatedSubject = try await translateText(subject, language: subjectLanguage ?? bodyLanguage)
            let translatedBody = try await translateText(body, language: bodyLanguage ?? subjectLanguage)
            try Task.checkCancellation()
            finish(status: 0, translatedSubject: translatedSubject, translatedBody: translatedBody)
        } catch MailTranslationError.missingModels, TranslationError.notInstalled {
            finish(status: 1, error: "本机翻译语言包不可用，请先在 macOS 系统翻译中下载邮件语言和简体中文")
        } catch MailTranslationError.unsupportedLanguage,
                TranslationError.unsupportedSourceLanguage,
                TranslationError.unsupportedTargetLanguage,
                TranslationError.unsupportedLanguagePairing {
            finish(status: 1, error: "macOS 系统翻译暂不支持这封邮件的语言")
        } catch MailTranslationError.unidentifiedLanguage, TranslationError.unableToIdentifyLanguage {
            finish(status: 1, error: "无法识别这封邮件的语言，请直接查看原文")
        } catch is CancellationError {
            finish(status: 2, error: "翻译已取消")
        } catch TranslationError.alreadyCancelled {
            finish(status: 2, error: "翻译已取消")
        } catch {
            finish(status: 1, error: "本机翻译暂时不可用，请稍后重试")
        }
    }

    @available(macOS 26.0, *)
    private func translateText(_ text: String, language: NLLanguage?) async throws -> String {
        try Task.checkCancellation()
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return text }
        guard let language else { throw MailTranslationError.unidentifiedLanguage }
        if language == .simplifiedChinese || language == .traditionalChinese { return text }

        let source = Locale.Language(identifier: language.rawValue)
        let target = Locale.Language(identifier: "zh-Hans")
        switch await LanguageAvailability().status(from: source, to: target) {
        case .installed:
            break
        case .supported:
            throw MailTranslationError.missingModels
        case .unsupported:
            throw MailTranslationError.unsupportedLanguage
        @unknown default:
            throw MailTranslationError.unsupportedLanguage
        }
        try Task.checkCancellation()
        // installedSource 只使用已下载的语言包，不请求下载，也不回退到网络翻译。
        let session = TranslationSession(installedSource: source, target: target)
        cancelSession = { session.cancel() }
        defer { cancelSession = nil }

        var result = ""
        var start = text.startIndex
        while start < text.endIndex {
            try Task.checkCancellation()
            var end = text.index(start, offsetBy: 4000, limitedBy: text.endIndex) ?? text.endIndex
            // 长正文分段提交，尽量在换行处分开，不裁掉预览中的任何文字。
            if end < text.endIndex,
               let newline = text[start..<end].lastIndex(of: "\n"),
               text.distance(from: start, to: newline) > 1000 {
                end = text.index(after: newline)
            }
            let part = String(text[start..<end])
            let content = part.trimmingCharacters(in: .whitespacesAndNewlines)
            if content.isEmpty {
                result += part
            } else {
                let prefix = String(part.prefix(while: { $0.isWhitespace }))
                let suffix = String(part.reversed().prefix(while: { $0.isWhitespace }).reversed())
                do {
                    let translated = try await session.translate(content).targetText
                    result += prefix + translated + suffix
                } catch TranslationError.nothingToTranslate {
                    // 数字、网址等短字段可能没有可译内容，应原样保留。
                    result += part
                }
            }
            start = end
        }
        return result
    }

    private func finish(status: Int32, translatedSubject: String = "", translatedBody: String = "", error: String = "") {
        guard !finished else { return }
        finished = true
        task = nil
        cancelSession = nil
        // Rust 只在这次同步回调中读取字符串，并在返回前回收自己的 Context。
        translatedSubject.withCString { subject in
            translatedBody.withCString { body in
                error.withCString { error in
                    callback(context, status, subject, body, error)
                }
            }
        }
    }
}

@_cdecl("poni_translation_start")
func poniTranslationStart(
    _ subject: UnsafePointer<CChar>,
    _ body: UnsafePointer<CChar>,
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, Int32, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Void,
    _ context: UnsafeMutableRawPointer?
) -> UnsafeMutableRawPointer {
    // 入口返回前复制输入，异步任务不借用 Rust 的临时字符串。
    let request = MailTranslationRequest(subject: String(cString: subject), body: String(cString: body), callback: callback, context: context)
    let handle = Unmanaged.passRetained(request).toOpaque()
    Task { @MainActor in request.start() }
    return handle
}

@_cdecl("poni_translation_cancel")
func poniTranslationCancel(_ handle: UnsafeMutableRawPointer) {
    let request = Unmanaged<MailTranslationRequest>.fromOpaque(handle).takeRetainedValue()
    Task { @MainActor in request.cancel() }
}
