// The offline speech engine on Apple Silicon: Nemotron Speech Streaming (560 ms) as Core ML, on
// the Neural Engine through FluidAudio. Measured on an 8 GB M1 playing the service in Safari
// (2026-10-27): ~4% of one core live on singing, against ~155% for the same model on the CPU
// (sherpa-onnx); detection on the eval sermons the same or better (replay --offline).
//
// It speaks the speech worker's protocol (kairo-server/src/offline.rs, run_worker), so the
// server runs it exactly as it runs that one:
//   kairo-coreml-speech <model dir> <language|->
//   stdin: u32 length (little-endian) + 16 kHz 16-bit mono PCM; a zero length finishes;
//          u32::MAX + one byte: singing heard (1) or not (0).
//   stdout: "K1 ready", then "K1 " + JSON per line: {"k":"i"|"l"|"f","t":…} what was heard
//          (interim, locked, final; this engine sends no locked words), {"k":"s","ms":…,"sung":…}
//          audio dropped to stay live.
// KAIRO_SPEECH_ACK=1 (offline-transcribe only): {"k":"a"} after each frame, so a recording is
// transcribed on its own clock.

import AVFoundation
import CoreML
import FluidAudio
import Foundation

let sampleRate = 16_000.0
/// The recogniser restarts its context at a pause once this many words are in it.
let resetAfterWords = 300
/// An utterance ends after this long with no new word (offline.rs UTTERANCE_END_SECS).
let utteranceEndSecs = 1.2
/// The model decodes in chunks of this many samples (560 ms).
let chunkSamples = 8_960
/// Audio waiting longer than this is dropped and recognition starts at the newest (offline.rs).
let maxBehind = 4.0
let maxBehindSinging = 1.0
let singingFrame = UInt32.max

let out = FileHandle.standardOutput
func say(_ line: String) {
    out.write(("K1 " + line + "\n").data(using: .utf8)!)
}
func say(_ obj: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: obj), let s = String(data: data, encoding: .utf8) else { return }
    say(s)
}

enum Heard {
    case interim(String), final(String)
    func emit() {
        switch self {
        case .interim(let t): say(["k": "i", "t": t])
        case .final(let t): say(["k": "f", "t": t])
        }
    }
}

/// One recognition stream: audio in, what was heard out. Unlike the ONNX engine (offline.rs) no
/// words are locked in while a sentence runs on: this recogniser never revises a word once it has
/// one, and its words come a chunk at a time, so locking cut sentences into pieces detection read
/// worse. Measured through this program on the two eval sermons (offline-transcribe --coreml,
/// 2026-10-27): locked, 59 verses found and 1 wrong; not locked, 62 found (the ONNX engine: 62)
/// and the two "wrong" both verses the preacher read that the answer key doesn't list
/// (Habakkuk 3:2, Haggai 2:3).
final class Transcriber {
    let engine: StreamingNemotronAsrManager
    let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: 1, interleaved: false)!
    /// Words of the recogniser's text already sent as finals.
    var locked = 0
    var lastPartial = ""
    /// Samples fed since the recogniser last started over.
    var fed = 0
    /// Tokens heard when the last utterance ended.
    var tokensAtEnd = 0

    init(engine: StreamingNemotronAsrManager) { self.engine = engine }

    func accept(_ samples: [Float]) async throws {
        guard let buf = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(samples.count)) else { return }
        buf.frameLength = AVAudioFrameCount(samples.count)
        samples.withUnsafeBufferPointer { buf.floatChannelData![0].update(from: $0.baseAddress!, count: samples.count) }
        try await engine.appendAudio(buf)
        try await engine.processBufferedAudio()
        fed += samples.count
    }

    func words() async -> [String] {
        await engine.getPartialTranscript().split(whereSeparator: \.isWhitespace).map(String.init)
    }

    func startOver() async {
        await engine.reset()
        locked = 0
        fed = 0
        tokensAtEnd = 0
        lastPartial = ""
    }

    /// Says what changed since last asked.
    func pump() async -> [Heard] {
        let words = await words()
        locked = min(locked, words.count)
        let timings = await engine.getTokenTimings()
        // What has been decoded: whole chunks only.
        let decoded = Double(fed - fed % chunkSamples) / sampleRate
        // Ended: 1.2 s decoded past the last word's end (ending 1.2 s after the last new word
        // instead made longer sentences, and found fewer verses: 60 against 62).
        if timings.count > tokensAtEnd, let last = timings.last, decoded - last.endTime >= utteranceEndSecs {
            var heard: [Heard] = []
            let rest = words[locked...].joined(separator: " ")
            if !rest.isEmpty { heard.append(.final(rest)) }
            lastPartial = ""
            locked = words.count
            tokensAtEnd = timings.count
            if words.count >= resetAfterWords { await startOver() }
            return heard
        }
        let live = words[locked...].joined(separator: " ")
        guard !live.isEmpty && live != lastPartial else { return [] }
        lastPartial = live
        return [.interim(live)]
    }

    /// Recognition fell behind and the waiting audio was dropped: what was heard so far ends as a
    /// final, and the recogniser starts fresh at the next audio.
    func cut() async -> Heard? {
        let words = await words()
        let rest = words[min(locked, words.count)...].joined(separator: " ")
        await startOver()
        return rest.isEmpty ? nil : .final(rest)
    }

    /// The audio has ended: whatever is left, as a final.
    func finish() async -> Heard? {
        let text = (try? await engine.finish()) ?? ""
        let words = text.split(whereSeparator: \.isWhitespace).map(String.init)
        let rest = words[min(locked, words.count)...].joined(separator: " ")
        return rest.isEmpty ? nil : .final(rest)
    }
}

/// What the reader hands the decoder: audio with when it arrived, or the end.
final class Inbox: @unchecked Sendable {
    private var items: [(Date, [Float])?] = []
    private let cond = NSCondition()
    var singing = false
    var gone = false

    func put(_ item: (Date, [Float])?) {
        cond.lock(); items.append(item); cond.signal(); cond.unlock()
    }
    /// The next item, waiting for one.
    func take() -> (Date, [Float])?? {
        cond.lock(); defer { cond.unlock() }
        while items.isEmpty { cond.wait() }
        return items.removeFirst()
    }
    /// Everything still waiting (audio samples dropped, and whether the end was among them).
    func drain() -> (Int, Bool) {
        cond.lock(); defer { cond.unlock() }
        var n = 0
        var ended = false
        while !items.isEmpty {
            if let item = items.removeFirst() { n += item.1.count } else { ended = true; break }
        }
        return (n, ended)
    }
}

func readExactly(_ n: Int) -> Data? {
    var data = Data()
    while data.count < n {
        let chunk = FileHandle.standardInput.readData(ofLength: n - data.count)
        if chunk.isEmpty { return nil }
        data.append(chunk)
    }
    return data
}

func run() async -> Int32 {
    let args = CommandLine.arguments
    guard args.count >= 2 else {
        FileHandle.standardError.write("usage: kairo-coreml-speech <model dir> <language|->\n".data(using: .utf8)!)
        return 2
    }
    // Live speech goes before a browser tab or video player on a busy media machine (offline.rs).
    pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0)
    // Kairo gone (quit, updated, crashed) while the model loads or the line is quiet: nothing will
    // close stdin to end this, so go too. An update's restart left one running alone (2026-10-30).
    let parent = getppid()
    Thread.detachNewThread {
        while true {
            Thread.sleep(forTimeInterval: 1)
            if getppid() != parent { exit(0) }
        }
    }
    // The CPU, unless KAIRO_COMPUTE_UNITS says otherwise ("ane": the Neural Engine, "gpu",
    // "all"). On an 8 GB M1 with macOS 27.2 (2026-10-30), 60 s of sermon took 12–15 s on the CPU
    // (16 s of CPU work, 1.3 GB at most), and 40–48 s on the Neural Engine (30 s of CPU work,
    // 2.2 GB), which also needs macOS to spend 6 minutes or more preparing the model after every
    // macOS or Kairo update; the same words either way. Live it fell behind and dropped audio.
    let units: MLComputeUnits
    switch ProcessInfo.processInfo.environment["KAIRO_COMPUTE_UNITS"] {
    case "ane": units = .cpuAndNeuralEngine
    case "gpu": units = .cpuAndGPU
    case "all": units = .all
    default: units = .cpuOnly
    }
    let engine = StreamingNemotronAsrManager(configuration: MLModelConfigurationUtils.defaultConfiguration(computeUnits: units))
    do {
        try await engine.loadModels(from: URL(fileURLWithPath: args[1]))
    } catch {
        FileHandle.standardError.write("[Offline] \(error.localizedDescription)\n".data(using: .utf8)!)
        return 2
    }
    let ack = ProcessInfo.processInfo.environment["KAIRO_SPEECH_ACK"] == "1"
    say("ready")

    let inbox = Inbox()
    let reader = Thread {
        while true {
            guard let len = readExactly(4) else { inbox.gone = true; inbox.put(nil); return }
            let n = len.withUnsafeBytes { $0.loadUnaligned(as: UInt32.self) }.littleEndian
            if n == singingFrame {
                guard let on = readExactly(1) else { inbox.gone = true; inbox.put(nil); return }
                inbox.singing = on[0] == 1
                continue
            }
            // A zero length is the server's "finish": the last words still matter.
            if n == 0 { inbox.put(nil); return }
            guard let pcm = readExactly(Int(n)) else { inbox.gone = true; inbox.put(nil); return }
            let samples = pcm.withUnsafeBytes { raw in
                raw.bindMemory(to: Int16.self).map { Float(Int16(littleEndian: $0)) / 32768.0 }
            }
            inbox.put((Date(), samples))
        }
    }
    reader.qualityOfService = .userInitiated
    reader.start()

    let heard = Transcriber(engine: engine)
    while true {
        guard let item = inbox.take(), let (arrived, samples) = item else {
            // Closed: no one is listening any more. Finished: the last words.
            if !inbox.gone, let h = await heard.finish() { h.emit() }
            return 0
        }
        let sung = inbox.singing
        let waited = Date().timeIntervalSince(arrived)
        if waited > maxBehind || (sung && waited > maxBehindSinging) {
            // Behind live speech: drop what is waiting, say so, start fresh at the newest.
            let (more, ended) = inbox.drain()
            if let h = await heard.cut() { h.emit() }
            say(["k": "s", "ms": (samples.count + more) * 1000 / Int(sampleRate), "sung": sung])
            if ended { return 0 }
            continue
        }
        do {
            try await heard.accept(samples)
        } catch {
            FileHandle.standardError.write("[Offline] \(error.localizedDescription)\n".data(using: .utf8)!)
            return 3
        }
        for h in await heard.pump() { h.emit() }
        if ack { say(["k": "a"]) }
    }
}

exit(await run())
