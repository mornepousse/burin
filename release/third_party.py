"""The crates inside the program, with their licences, from `cargo metadata` on standard input."""
import json
import sys

meta = json.load(sys.stdin)
used = {node["id"] for node in meta["resolve"]["nodes"]}
print("Burin is built with these open source components, each under its own licence.\n")
for name, version, licence in sorted((p["name"], p["version"], p.get("license") or "see its own files") for p in meta["packages"] if p["id"] in used and p["name"] != "burin"):
    print(f"{name} {version}: {licence}")
