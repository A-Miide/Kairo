// kairo-notes-writer: writes sermon notes with the Mac's own on-device model (Apple Intelligence,
// FoundationModels), for Apple-silicon Macs on macOS 26 or later. Kairo splits the sermon into
// sections and finds each section's scriptures; this program only puts each section into words.
// Nothing is downloaded and nothing leaves the Mac.
//
//   kairo-notes-writer --check            {"available":true} or {"available":false,"reason":"…"}
//   kairo-notes-writer < sermon.json      progress lines, then one {"done":true,…} line
//
// Input:  {"sections":[{"id":"s1","refs":["Isaiah 58:6"],"text":"…the section's transcript…"}]}
// Output: {"done":true,"title":"…","summary":"…","sections":[{"id":"s1","heading":"…","bullets":["…"]}]}
//         A section the model refused or garbled comes back with no bullets; Kairo outlines it instead.
import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

struct SectionIn: Codable { let id: String; let refs: [String]?; let text: String }
struct Input: Codable { let sections: [SectionIn] }
struct SectionOut: Codable { let id: String; let heading: String; let bullets: [String] }
struct Output: Codable { let done: Bool; let title: String; let summary: String; let sections: [SectionOut] }

func emit(_ obj: [String: Any]) {
    if let d = try? JSONSerialization.data(withJSONObject: obj), let s = String(data: d, encoding: .utf8) {
        print(s)
        fflush(stdout)
    }
}

/// A model's reply as a heading and bullets, whatever markdown it dressed them in.
func parseNote(_ raw: String) -> (heading: String, bullets: [String]) {
    var heading = ""
    var bullets: [String] = []
    for line in raw.replacingOccurrences(of: "<end_of_turn>", with: "").split(separator: "\n") {
        var l = line.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "*", with: "")
        while l.hasPrefix("#") { l.removeFirst() }
        l = l.trimmingCharacters(in: .whitespaces)
        if l.isEmpty { continue }
        if l.hasPrefix("-") || l.hasPrefix("•") {
            let b = l.drop(while: { $0 == "-" || $0 == "•" || $0 == " " }).trimmingCharacters(in: .whitespaces)
            if !b.isEmpty { bullets.append(b) }
        } else if heading.isEmpty {
            heading = l
            for p in ["HEADING:", "Heading:", "heading:"] where heading.hasPrefix(p) {
                heading = String(heading.dropFirst(p.count)).trimmingCharacters(in: .whitespaces)
            }
        }
    }
    return (heading, Array(bullets.prefix(4)))
}

/// A section's transcript in pieces of about 380 words, each ending at a sentence end when it can.
func pieces(_ text: String) -> [String] {
    let words = text.split(whereSeparator: { $0 == " " || $0 == "\n" }).map(String.init)
    var out: [String] = []
    var cur: [String] = []
    for w in words {
        cur.append(w)
        let ends = w.hasSuffix(".") || w.hasSuffix("?") || w.hasSuffix("!")
        if (cur.count >= 340 && ends) || cur.count >= 420 {
            out.append(cur.joined(separator: " "))
            cur = []
        }
    }
    if cur.count >= 40 || out.isEmpty { out.append(cur.joined(separator: " ")) }
    else if !cur.isEmpty { out[out.count - 1] += " " + cur.joined(separator: " ") }
    return out
}

#if canImport(FoundationModels)
@available(macOS 26.0, *)
func unavailableReason() -> String? {
    switch SystemLanguageModel.default.availability {
    case .available: return nil
    case .unavailable(let why): return "\(why)"
    }
}

@available(macOS 26.0, *)
func run(_ input: Input) async {
    // The looser setting is made for rewriting people's own text: sermons talk of demons, death and
    // curses, and the default filter refused about a third of them.
    let model = SystemLanguageModel(guardrails: .permissiveContentTransformations)
    let system = "You write sermon notes from an automatic speech transcript that contains recognition errors. Infer what the preacher meant and ignore garbled fragments. Write in third person. Only say what the text supports; never add scriptures or facts that are not in it."
    func ask(_ prompt: String) async -> String? {
        let session = LanguageModelSession(model: model, instructions: system)
        return try? await session.respond(to: prompt).content
    }
    let total = input.sections.reduce(0) { $0 + pieces($1.text).count + (pieces($1.text).count > 1 ? 1 : 0) } + 1
    var done = 0
    func tick() { done += 1; emit(["progress": done, "of": total]) }
    var outs: [SectionOut] = []
    for sec in input.sections {
        let hint = (sec.refs ?? []).isEmpty ? "" : "\nScriptures read or cited in this excerpt: \((sec.refs ?? []).prefix(6).joined(separator: ", ")).\n"
        var minis: [(heading: String, bullets: [String])] = []
        for p in pieces(sec.text) {
            let prompt = "Excerpt of a sermon transcript:\n\"\"\"\n\(p)\n\"\"\"\n\(hint)\nWrite the notes for this excerpt in exactly this form:\nHEADING: a section heading of 3 to 6 words\n- first bullet, one plain sentence\n- second bullet\n- optional third bullet\nNo quotes from the transcript."
            if let r = await ask(prompt) {
                let n = parseNote(r)
                if !n.heading.isEmpty && !n.bullets.isEmpty { minis.append(n) }
            }
            tick()
        }
        if minis.count <= 1 {
            let n = minis.first ?? ("", [])
            outs.append(SectionOut(id: sec.id, heading: n.heading, bullets: n.bullets))
            continue
        }
        let notes = minis.map { "- " + $0.heading + ": " + $0.bullets.joined(separator: " ") }.joined(separator: "\n")
        let prompt = "Notes taken from consecutive parts of one section of a sermon:\n\(notes)\n\(hint)\nMerge them into ONE set of notes for this whole section. Output exactly one HEADING line and then exactly 4 bullets, nothing else. Do not repeat the sub-headings; combine overlapping points into the 4 strongest. Form:\nHEADING: a section heading of 3 to 6 words\n- bullet\n- bullet\n- bullet\n- bullet\nEach bullet one plain sentence in third person. Keep only what the notes say."
        var merged = minis[0]
        if let r = await ask(prompt) {
            let n = parseNote(r)
            if !n.heading.isEmpty && n.bullets.count >= 2 { merged = n }
        }
        tick()
        outs.append(SectionOut(id: sec.id, heading: merged.heading, bullets: merged.bullets))
    }
    var title = ""
    var summary = ""
    let heads = outs.filter { !$0.bullets.isEmpty }.map { "- " + $0.heading + ": " + ($0.bullets.first ?? "") }.joined(separator: "\n")
    if !heads.isEmpty, let r = await ask("Section notes of one sermon:\n\(heads)\n\nWrite exactly two lines and nothing else, starting with the labels TITLE: and SUMMARY:\nTITLE: a sermon title of 2 to 6 words\nSUMMARY: one sentence that sums up the whole sermon") {
        for line in r.split(separator: "\n") {
            let l = line.replacingOccurrences(of: "*", with: "").trimmingCharacters(in: .whitespaces)
            if l.uppercased().hasPrefix("TITLE:") { title = String(l.dropFirst(6)).trimmingCharacters(in: .whitespaces) }
            if l.uppercased().hasPrefix("SUMMARY:") { summary = String(l.dropFirst(8)).trimmingCharacters(in: .whitespaces) }
        }
    }
    tick()
    if let d = try? JSONEncoder().encode(Output(done: true, title: title, summary: summary, sections: outs)), let s = String(data: d, encoding: .utf8) {
        print(s)
        fflush(stdout)
    }
}
#endif

func unavailable(_ reason: String) -> Never {
    emit(["available": false, "reason": reason])
    exit(CommandLine.arguments.contains("--check") ? 0 : 1)
}

#if canImport(FoundationModels)
if #available(macOS 26.0, *) {
    if let why = unavailableReason() { unavailable(why) }
    if CommandLine.arguments.contains("--check") { emit(["available": true]); exit(0) }
    let data = FileHandle.standardInput.readDataToEndOfFile()
    guard let input = try? JSONDecoder().decode(Input.self, from: data) else { emit(["error": "unreadable input"]); exit(2) }
    await run(input)
} else {
    unavailable("needs macOS 26 or later")
}
#else
unavailable("built without Apple Intelligence support")
#endif
