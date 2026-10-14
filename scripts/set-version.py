#!/usr/bin/env python3
"""Ustawia wersję aplikacji we wszystkich plikach (tauri.conf.json, Cargo.toml, package.json)."""
import json, pathlib, re, sys

v = sys.argv[1].lstrip("v")
if not re.fullmatch(r"\d+\.\d+\.\d+", v):
    sys.exit(f"Nieprawidłowa wersja: {v}")
root = pathlib.Path(__file__).resolve().parent.parent
for name in ("src-tauri/tauri.conf.json", "package.json"):
    p = root / name
    data = json.loads(p.read_text(encoding="utf-8"))
    data["version"] = v
    p.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
cargo = root / "src-tauri" / "Cargo.toml"
text = cargo.read_text(encoding="utf-8")
text = re.sub(r'(?m)^version = "[^"]+"', f'version = "{v}"', text, count=1)
cargo.write_text(text, encoding="utf-8")
print("Wersja:", v)
