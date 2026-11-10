#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Validating manifests across all plugin ecosystems..."

# 1. Extract Cargo version
CARGO_VERSION=$(grep '^version = ' Cargo.toml | head -n1 | cut -d '"' -f2)
echo "  • Detected Cargo version: $CARGO_VERSION"

# 2. Check JSON validity
JSON_FILES=(
  ".claude-plugin/plugin.json"
  ".claude-plugin/marketplace.json"
  ".codex-plugin/plugin.json"
  "gemini-extension.json"
  "server.json"
  ".mcp.json"
)

for f in "${JSON_FILES[@]}"; do
  if [ ! -f "$f" ]; then
    echo "  ✖ Missing JSON manifest: $f" >&2
    exit 1
  fi
  python3 -c "import json; json.load(open('$f'))" || {
    echo "  ✖ Invalid JSON syntax in $f" >&2
    exit 1
  }
  echo "  ✔ Valid JSON: $f"
done

# 3. Check YAML validity
if [ ! -f "plugin.yaml" ]; then
  echo "  ✖ Missing plugin.yaml" >&2
  exit 1
fi
python3 -c "
import sys
for line in open('plugin.yaml'):
    if line.startswith('version:'):
        v = line.split(':')[1].strip()
        if v != '$CARGO_VERSION':
            sys.exit(f'Version mismatch in plugin.yaml: {v} != $CARGO_VERSION')
" || {
  echo "  ✖ Version mismatch in plugin.yaml" >&2
  exit 1
}
echo "  ✔ Valid YAML and version: plugin.yaml ($CARGO_VERSION)"

# 4. Check version synchronization across all 6 files
python3 -c "
import json, sys

cargo_ver = '$CARGO_VERSION'

# .claude-plugin/plugin.json
with open('.claude-plugin/plugin.json') as f:
    d = json.load(f)
    if d.get('version') != cargo_ver:
        sys.exit(f'.claude-plugin/plugin.json version {d.get(\"version\")} != {cargo_ver}')

# .codex-plugin/plugin.json
with open('.codex-plugin/plugin.json') as f:
    d = json.load(f)
    if d.get('version') != cargo_ver:
        sys.exit(f'.codex-plugin/plugin.json version {d.get(\"version\")} != {cargo_ver}')

# gemini-extension.json
with open('gemini-extension.json') as f:
    d = json.load(f)
    if d.get('version') != cargo_ver:
        sys.exit(f'gemini-extension.json version {d.get(\"version\")} != {cargo_ver}')

# server.json
with open('server.json') as f:
    d = json.load(f)
    if d.get('version') != cargo_ver:
        sys.exit(f'server.json version {d.get(\"version\")} != {cargo_ver}')
    pkg_ver = d.get('packages', [{}])[0].get('version')
    if pkg_ver != cargo_ver:
        sys.exit(f'server.json package version {pkg_ver} != {cargo_ver}')

print('  ✔ All manifest versions match Cargo.toml exactly: ' + cargo_ver)
"

# 5. Verify local path references
[ -d "skills/quad" ] || { echo "  ✖ Missing skills/quad directory" >&2; exit 1; }
[ -f "skills/quad/SKILL.md" ] || { echo "  ✖ Missing skills/quad/SKILL.md" >&2; exit 1; }
[ -f "AGENTS.md" ] || { echo "  ✖ Missing AGENTS.md context file" >&2; exit 1; }
echo "  ✔ Internal path references verified"

echo "==> All manifest and deployment checks succeeded!"

