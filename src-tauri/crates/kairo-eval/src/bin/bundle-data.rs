//! Puts in place what the app bundles but git doesn't hold: the meaning
//! models (bge-small-en-v1.5, the MiniLM cross-encoder — pinned revisions,
//! checksum-verified) and the verse index built from them for this
//! numbering of the Bible. Run before bundling (`tauri.conf.json`'s
//! beforeBuildCommand) and once on a fresh checkout; anything already right
//! is left alone.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use kairo_eval::{meaning, root};
use sha2::{Digest, Sha256};

struct File {
    repo: &'static str,
    revision: &'static str,
    path: &'static str,
    /// Under databases/bibles.
    local: &'static str,
    size: u64,
    sha256: &'static str,
}

const BGE: &str = "Xenova/bge-small-en-v1.5";
const BGE_REV: &str = "ea104dacec62c0de699686887e3f920caeb4f3e3";
const MINILM: &str = "Xenova/ms-marco-MiniLM-L-12-v2";
const MINILM_REV: &str = "42a4a787e30451cf9dbd09080c2a5b8dde332c1e";

const FILES: &[File] = &[
    File { repo: BGE, revision: BGE_REV, path: "config.json", local: "model/Xenova/bge-small-en-v1.5/config.json", size: 683, sha256: "fa73f90bf92c8cace1fbcb709626306f2bdbc9ea3e5b5f94b440df9b6aa56350" },
    File { repo: BGE, revision: BGE_REV, path: "tokenizer.json", local: "model/Xenova/bge-small-en-v1.5/tokenizer.json", size: 711396, sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66" },
    File { repo: BGE, revision: BGE_REV, path: "tokenizer_config.json", local: "model/Xenova/bge-small-en-v1.5/tokenizer_config.json", size: 366, sha256: "9261e7d79b44c8195c1cada2b453e55b00aeb81e907a6664974b4d7776172ab3" },
    File { repo: BGE, revision: BGE_REV, path: "onnx/model_quantized.onnx", local: "model/Xenova/bge-small-en-v1.5/onnx/model_quantized.onnx", size: 34014426, sha256: "6c9c6101a956d62dfb5e7190c538226c0c5bb9cb27b651234b6df063ee7dbfe4" },
    File { repo: MINILM, revision: MINILM_REV, path: "config.json", local: "reranker_model/Xenova/ms-marco-MiniLM-L-12-v2/config.json", size: 826, sha256: "681b15ee0e4ec2cc2320b4e32e01266ebcbc8a84d55c9f0fac04cfefb7eece30" },
    File { repo: MINILM, revision: MINILM_REV, path: "tokenizer.json", local: "reranker_model/Xenova/ms-marco-MiniLM-L-12-v2/tokenizer.json", size: 711396, sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66" },
    File { repo: MINILM, revision: MINILM_REV, path: "tokenizer_config.json", local: "reranker_model/Xenova/ms-marco-MiniLM-L-12-v2/tokenizer_config.json", size: 1242, sha256: "0b29c7bfc889e53b36d9dd3e686dd4300f6525110eaa98c76a5dafceb2029f53" },
    File { repo: MINILM, revision: MINILM_REV, path: "onnx/model_int8.onnx", local: "reranker_model/Xenova/ms-marco-MiniLM-L-12-v2/onnx/model_int8.onnx", size: 33940752, sha256: "4509e8d29fce1db0ecdf9413c4269ccd7787fc925ecd2bdd3fd2fe99bc9b6b6f" },
];

fn sha256(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn right(path: &Path, f: &File) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == f.size) && sha256(path).as_deref() == Some(f.sha256)
}

fn fetch(f: &File, dest: &Path) {
    std::fs::create_dir_all(dest.parent().unwrap()).expect("model folder");
    let url = format!("https://huggingface.co/{}/resolve/{}/{}", f.repo, f.revision, f.path);
    println!("fetching {} ({:.1} MB)", f.local, f.size as f64 / 1e6);
    let partial = dest.with_extension("part");
    // curl ships with macOS, Windows 10+ and every CI image.
    let ok = Command::new("curl").args(["-fSL", "--retry", "5", "--retry-delay", "3", "-o"]).arg(&partial).arg(&url).status().is_ok_and(|s| s.success());
    if !ok {
        panic!("download failed: {url}");
    }
    std::fs::rename(&partial, dest).expect("move download into place");
    if !right(dest, f) {
        let _ = std::fs::remove_file(dest);
        panic!("{} did not match its checksum", f.local);
    }
}

fn main() {
    let bibles = root().join("databases/bibles");
    for f in FILES {
        let dest = bibles.join(f.local);
        if !right(&dest, f) {
            fetch(f, &dest);
        }
    }
    println!("meaning models present and verified");

    let target = bibles.join("verse_index.bge-small.kvx");
    let verses = kairo_bible::Corpus::load(&bibles).expect("kjv").len();
    // Present and built for this numbering of the Bible.
    if kairo_semantic::VerseIndex::load(&target).is_ok_and(|i| i.len() == verses) {
        println!("verse index present: {}", target.display());
        return;
    }
    // Builds (or reuses) the cached index, then copies it into place: a few
    // minutes for 31,102 verses with bge-small, so no prebuilt copy is kept.
    let _ = meaning("bge-small.json", None, &["KJV"]);
    let cached = root().join("eval-results/semantic-cache/bge-small.KJV.kvx");
    std::fs::copy(&cached, &target).expect("copy verse index");
    println!("verse index built: {}", target.display());
}
