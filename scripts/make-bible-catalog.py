#!/usr/bin/env python3
"""Builds the downloadable Bible catalogue from the packs in databases/bibles/packs.

    python3 scripts/make-bible-catalog.py [--out dist/bible-packs] [--base URL] [--lang pt,es]

Writes <out>/catalog.json and copies each pack file next to it. Upload those files as the
assets of one GitHub release tagged `bible-packs`; the app reads
https://github.com/<owner>/<repo>/releases/download/bible-packs/catalog.json (override with
KAIRO_BIBLE_CATALOG_URL). Each entry carries the pack's translation name and licence so the
app can show its credit. Nothing is uploaded by this script.

Only open-licence texts belong here (public domain, CC BY-SA, GPL text packs): the
licence is read from each pack file and printed, so it can be checked before publishing.
"""
import argparse
import hashlib
import json
import os
import shutil
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
PACKS = os.path.join(ROOT, "databases", "bibles", "packs")
DEFAULT_BASE = "https://github.com/miide-Adeyemi/Kairo/releases/download/bible-packs"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(ROOT, "dist", "bible-packs"))
    ap.add_argument("--base", default=DEFAULT_BASE, help="folder URL the files will be published under")
    ap.add_argument("--lang", default="", help="comma list of language codes (default: all packs)")
    a = ap.parse_args()
    want = {l.strip().lower() for l in a.lang.split(",") if l.strip()}
    os.makedirs(a.out, exist_ok=True)
    packs = []
    for name in sorted(os.listdir(PACKS)):
        if not name.endswith(".json") or name == "catalog.json":
            continue
        lang = name[:-5]
        if want and lang not in want:
            continue
        path = os.path.join(PACKS, name)
        raw = open(path, "rb").read()
        meta = json.loads(raw)
        if not meta.get("verses"):
            sys.exit(f"{name}: no verses")
        packs.append({
            "lang": lang,
            "name": meta.get("language", lang),
            "translation": meta.get("translation", ""),
            "license": meta.get("license", ""),
            "source": meta.get("source", ""),
            "bytes": len(raw),
            "sha256": hashlib.sha256(raw).hexdigest(),
            "url": f"{a.base.rstrip('/')}/{name}",
        })
        shutil.copyfile(path, os.path.join(a.out, name))
    json.dump({"version": 1, "packs": packs}, open(os.path.join(a.out, "catalog.json"), "w"), indent=1, ensure_ascii=False)
    for p in packs:
        print(f"{p['lang']:>4}  {p['bytes'] / 1e6:5.1f} MB  {p['license']:<28} {p['translation']}")
    print(f"\nWrote {len(packs)} packs and catalog.json to {a.out}")
    print("Publish with (after checking the licences above):")
    print(f"  gh release create bible-packs {a.out}/*.json --title 'Bible packs' --notes 'Downloadable Bibles for Kairo'")


if __name__ == "__main__":
    main()
