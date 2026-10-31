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
//   kairo-coreml-speech --compare <encoder.mlmodelc> <encoder.mlmodelc>: how far apart the two
//   encoders' outputs are on the same input, "K1 compare <difference>"; exits 0 when within
//   `compareTolerance` (kairo-server coreml_encoder.rs, before it keeps a rewritten encoder).

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
/// Audio waiting longer than this is dropped and recognition starts at the newest, unless this
/// computer decodes fast enough to catch up (`Pace`); then only past `maxBehindHard` (offline.rs).
let maxBehind = 4.0
let maxBehindHard = 15.0
let maxBehindSinging = 1.0
/// Decoding at least this much faster than real time catches up on a backlog instead of dropping it.
let minPace = 1.25
/// The pace is measured over this much of the latest audio.
let paceWindow = 30.0

/// How fast this computer decodes: seconds of audio per second spent decoding, over the latest
/// `paceWindow` of audio. A stall (the disk, a busy moment) barely moves it; a computer that can't
/// keep up shows it within half a minute.
struct Pace {
    private var spans: [(audio: Double, busy: Double)] = []
    private var audio = 0.0
    private var busy = 0.0
    mutating func add(audio a: Double, busy b: Double) {
        spans.append((a, b))
        audio += a
        busy += b
        while let first = spans.first, audio - first.audio >= paceWindow {
            audio -= first.audio
            busy -= first.busy
            spans.removeFirst()
        }
    }
    var value: Double { busy > 0 ? audio / busy : .infinity }
}

/// The catch-up rule (offline.rs too_far_behind): audio that arrived `waited` seconds ago and isn't
/// decoded yet is dropped only when this computer can't catch up on it, or it's too old to use.
func tooFarBehind(_ waited: Double, pace: Double) -> Bool {
    waited > maxBehindHard || (waited > maxBehind && pace < minPace)
}
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

/// Locks the compiled model into memory for as long as this program runs; returns the bytes locked.
///
/// Core ML doesn't decode from the model's weight files: it compiles them once into its cache
/// (com.apple.e5rt.e5bundlecache, 1.1 GB for the encoder on the CPU) and maps that, and the
/// encoder reads all of it every chunk. Mapped from a file, those are clean pages, the first thing
/// macOS takes back when memory runs short, so decoding waited on the disk. Measured on an 8 GB M1
/// listening to YouTube in Safari through BlackHole (2026-10-31): 740 MB of the 1.1 GB in memory,
/// ~800 MB read back from the disk every 10 s, and 54 s of 80 s of speech dropped. The ONNX engine
/// locks its weights the same way (onnx_weights.rs, pin).
///
/// Only a mapping of its own is locked: the files Core ML mapped are found by walking this
/// process's regions, then opened and mapped again read-only here, which locks the same pages.
/// Locking Core ML's regions in place instead panicked the kernel on macOS 27.2 (2026-10-31): a
/// region's file name comes from the next region with a file, so a region without one (the
/// commpage) was locked too.
func pinCompiledModel() -> Int {
    let me = getpid()
    var files = Set<String>()
    var address: UInt64 = 0
    var info = proc_regionwithpathinfo()
    let infoSize = Int32(MemoryLayout<proc_regionwithpathinfo>.stride)
    while proc_pidinfo(me, PROC_PIDREGIONPATHINFO, address, &info, infoSize) == infoSize {
        let path = withUnsafeBytes(of: info.prp_vip.vip_path) { String(cString: $0.bindMemory(to: CChar.self).baseAddress!) }
        if path.contains("e5bundlecache") { files.insert(path) }
        let next = info.prp_prinfo.pri_address + info.prp_prinfo.pri_size
        if next <= address { break }
        address = next
    }
    var locked = 0
    for path in files {
        let fd = open(path, O_RDONLY)
        guard fd >= 0 else { continue }
        defer { close(fd) }
        var st = stat()
        guard fstat(fd, &st) == 0, (st.st_mode & S_IFMT) == S_IFREG, st.st_size > 0 else { continue }
        let len = Int(st.st_size)
        // Kept mapped (and locked) until this program ends.
        guard let map = mmap(nil, len, PROT_READ, MAP_SHARED, fd, 0), map != MAP_FAILED else { continue }
        if mlock(map, len) == 0 { locked += len } else { munmap(map, len) }
    }
    return locked
}

/// The encoder's outputs, rewritten (coreml_encoder.rs) against as published, may differ by this
/// much (relative to the published one's size) on any chunk. Measured on an 8 GB M1 with macOS 27.2
/// (2026-10-31), 8 chunks: at most 2.3% for the rewrite Kairo ships (5% with all 193 linears
/// rewritten); one with its scales left out, 217%. An encoder computing wrong is off by about
/// 100%, or not a number.
let compareTolerance = 0.05
let compareChunks = 8

/// Runs both encoders on the same made-up mel chunks (each carrying its own state from chunk to
/// chunk, as decoding does) and says how far apart they came out: the largest difference over the
/// chunks, the distance between the outputs over the size of the first's.
func compareEncoders(_ a: String, _ b: String, units: MLComputeUnits) -> Int32 {
    let config = MLModelConfiguration()
    config.computeUnits = units
    func array(_ shape: [Int], _ type: MLMultiArrayDataType, _ fill: (Int) -> Double) throws -> MLMultiArray {
        let m = try MLMultiArray(shape: shape.map { NSNumber(value: $0) }, dataType: type)
        for i in 0..<m.count { m[i] = NSNumber(value: fill(i)) }
        return m
    }
    // The same numbers every time and on every Mac: a linear congruential generator, made normal.
    var seed: UInt64 = 20261031
    func uniform() -> Double {
        seed = seed &* 6364136223846793005 &+ 1442695040888963407
        return (Double(seed >> 11) + 0.5) / Double(1 << 53)
    }
    func normal() -> Double { sqrt(-2 * log(uniform())) * cos(2 * Double.pi * uniform()) }
    do {
        let models = try [a, b].map { try MLModel(contentsOf: URL(fileURLWithPath: $0), configuration: config) }
        var states = try models.map { _ in
            [
                "cache_channel": try array([1, 24, 70, 1024], .float32) { _ in 0 },
                "cache_len": try array([1], .int32) { _ in 0 },
                "cache_time": try array([1, 24, 1024, 8], .float32) { _ in 0 },
            ]
        }
        var worst = 0.0
        for _ in 0..<compareChunks {
            let mel = try array([1, 128, 65], .float32) { _ in 2 * normal() }
            let length = try array([1], .int32) { _ in 65 }
            var encoded: [MLMultiArray] = []
            for (i, model) in models.enumerated() {
                var inputs = states[i]
                inputs["mel"] = mel
                inputs["mel_length"] = length
                let out = try model.prediction(from: MLDictionaryFeatureProvider(dictionary: inputs))
                guard let e = out.featureValue(for: "encoded")?.multiArrayValue,
                      let channel = out.featureValue(for: "cache_channel_out")?.multiArrayValue,
                      let len = out.featureValue(for: "cache_len_out")?.multiArrayValue,
                      let time = out.featureValue(for: "cache_time_out")?.multiArrayValue
                else { throw CocoaError(.featureUnsupported) }
                encoded.append(e)
                states[i] = ["cache_channel": channel, "cache_len": len, "cache_time": time]
            }
            var (diff, size) = (0.0, 0.0)
            for j in 0..<encoded[0].count {
                let (x, y) = (encoded[0][j].doubleValue, encoded[1][j].doubleValue)
                diff += (x - y) * (x - y)
                size += x * x
            }
            // Not a number (NaN) fails too: NaN is never within the tolerance.
            let d = size > 0 ? sqrt(diff / size) : .infinity
            worst = d.isNaN || d > worst ? d : worst
        }
        say("compare \(worst)")
        return worst <= compareTolerance ? 0 : 1
    } catch {
        FileHandle.standardError.write("[Offline] \(error.localizedDescription)\n".data(using: .utf8)!)
        return 2
    }
}

/// The CPU, unless KAIRO_COMPUTE_UNITS says otherwise ("ane": the Neural Engine, "gpu",
/// "all"). On an 8 GB M1 with macOS 27.2 (2026-10-30), 60 s of sermon took 12–15 s on the CPU
/// (16 s of CPU work, 1.3 GB at most), and 40–48 s on the Neural Engine (30 s of CPU work,
/// 2.2 GB), which also needs macOS to spend 6 minutes or more preparing the model after every
/// macOS or Kairo update; the same words either way. Live it fell behind and dropped audio.
func computeUnits() -> MLComputeUnits {
    switch ProcessInfo.processInfo.environment["KAIRO_COMPUTE_UNITS"] {
    case "ane": return .cpuAndNeuralEngine
    case "gpu": return .cpuAndGPU
    case "all": return .all
    default: return .cpuOnly
    }
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
    let units = computeUnits()
    if args[1] == "--compare", args.count >= 4 {
        return compareEncoders(args[2], args[3], units: units)
    }
    let engine = StreamingNemotronAsrManager(configuration: MLModelConfigurationUtils.defaultConfiguration(computeUnits: units))
    do {
        try await engine.loadModels(from: URL(fileURLWithPath: args[1]))
    } catch {
        FileHandle.standardError.write("[Offline] \(error.localizedDescription)\n".data(using: .utf8)!)
        return 2
    }
    let pinned = pinCompiledModel()
    FileHandle.standardError.write((pinned > 0
        ? "[Offline] \(pinned >> 20) MB of the compiled model locked in memory\n"
        : "[Offline] the compiled model isn't locked in memory\n").data(using: .utf8)!)
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
    var pace = Pace()
    while true {
        guard let item = inbox.take(), let (arrived, samples) = item else {
            // Closed: no one is listening any more. Finished: the last words.
            if !inbox.gone, let h = await heard.finish() { h.emit() }
            return 0
        }
        let sung = inbox.singing
        let waited = Date().timeIntervalSince(arrived)
        if tooFarBehind(waited, pace: pace.value) || (sung && waited > maxBehindSinging) {
            // Behind live speech and not catching up: drop what is waiting, say so, start fresh
            // at the newest.
            let (more, ended) = inbox.drain()
            if let h = await heard.cut() { h.emit() }
            say(["k": "s", "ms": (samples.count + more) * 1000 / Int(sampleRate), "sung": sung])
            if ended { return 0 }
            continue
        }
        let started = Date()
        do {
            try await heard.accept(samples)
        } catch {
            FileHandle.standardError.write("[Offline] \(error.localizedDescription)\n".data(using: .utf8)!)
            return 3
        }
        for h in await heard.pump() { h.emit() }
        pace.add(audio: Double(samples.count) / sampleRate, busy: Date().timeIntervalSince(started))
        if ack { say(["k": "a"]) }
    }
}

exit(await run())
