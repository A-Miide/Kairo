//! Puts in place what the app bundles but git doesn't hold: the meaning
//! models (EmbeddingGemma, the MiniLM cross-encoder — pinned revisions,
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

const GEMMA: &str = "onnx-community/embeddinggemma-300m-ONNX";
const GEMMA_REV: &str = "5090578d9565bb06545b4552f76e6bc2c93e4a66";
const MINILM: &str = "Xenova/ms-marco-MiniLM-L-12-v2";
const MINILM_REV: &str = "42a4a787e30451cf9dbd09080c2a5b8dde332c1e";

const FILES: &[File] = &[
    File { repo: GEMMA, revision: GEMMA_REV, path: "config.json", local: "model/onnx-community/embeddinggemma-300m-ONNX/config.json", size: 1765, sha256: "6e1f06404b7163e0325ed2ea3e6781cde50f4a50b31780a95ad0d30e8404d77b" },
    File { repo: GEMMA, revision: GEMMA_REV, path: "tokenizer.json", local: "model/onnx-community/embeddinggemma-300m-ONNX/tokenizer.json", size: 20323312, sha256: "4dda02faaf32bc91031dc8c88457ac272b00c1016cc679757d1c441b248b9c47" },
    File { repo: GEMMA, revision: GEMMA_REV, path: "tokenizer_config.json", local: "model/onnx-community/embeddinggemma-300m-ONNX/tokenizer_config.json", size: 1156830, sha256: "3ca953eea6c3c9fcda9cf3df22949ff18b216f7c74bd6459230f3f1013953f3a" },
    File { repo: GEMMA, revision: GEMMA_REV, path: "onnx/model_q4.onnx", local: "model/onnx-community/embeddinggemma-300m-ONNX/onnx/model_q4.onnx", size: 519322, sha256: "ad1dfee81a70f7944b9b9d1cc6e48075b832881cf33fab2f2b248be78f3f0043" },
    File { repo: GEMMA, revision: GEMMA_REV, path: "onnx/model_q4.onnx_data", local: "model/onnx-community/embeddinggemma-300m-ONNX/onnx/model_q4.onnx_data", size: 196725760, sha256: "599962c3143b040de2dd05e5975be3e9091dd067cacc6a8f7186e3203bab9e02" },
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

/// The verse index, built once from these models and this KJV and kept as a
/// release file: a fresh build machine downloads it in seconds instead of
/// embedding 31,102 verses (40+ minutes on a CI runner). Checked by its
/// checksum and its verse count; anything else, and it's built here.
const PREBUILT_INDEX: &str = "https://github.com/miide-Adeyemi/Kairo/releases/download/bundle-data-v1/verse_index.gemma-legacy.kvx";
const PREBUILT_INDEX_SHA256: &str = "4dd710f743ebc1d722a940ea4a3eb3daf7f34ce83885d1c328de4bbfa8bc19db";

fn fetch_prebuilt_index(target: &Path, verses: usize) -> bool {
    let partial = target.with_extension("part");
    let ok = Command::new("curl").args(["-fsSL", "--retry", "3", "-o"]).arg(&partial).arg(PREBUILT_INDEX).status().is_ok_and(|s| s.success())
        && sha256(&partial).as_deref() == Some(PREBUILT_INDEX_SHA256)
        && kairo_semantic::VerseIndex::load(&partial).is_ok_and(|i| i.len() == verses);
    if ok {
        std::fs::rename(&partial, target).is_ok()
    } else {
        let _ = std::fs::remove_file(&partial);
        false
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

    let target = bibles.join("verse_index.gemma-legacy.kvx");
    let verses = kairo_bible::Corpus::load(&bibles).expect("kjv").len();
    // Present and built for this numbering of the Bible.
    if kairo_semantic::VerseIndex::load(&target).is_ok_and(|i| i.len() == verses) {
        println!("verse index present: {}", target.display());
        return;
    }
    if fetch_prebuilt_index(&target, verses) {
        println!("verse index downloaded and verified: {}", target.display());
        return;
    }
    // Builds (or reuses) the cached index, then copies it into place.
    let _ = meaning("gemma-legacy.json", None, &["KJV"]);
    let cached = root().join("eval-results/semantic-cache/gemma-legacy.KJV.kvx");
    std::fs::copy(&cached, &target).expect("copy verse index");
    println!("verse index built: {}", target.display());
}
