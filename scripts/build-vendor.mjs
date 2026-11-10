#!/usr/bin/env node
// Build the vendored fijii SDK if it isn't built yet.
//
// The submodule's own .gitignore excludes `dist/`, so a fresh checkout has only
// source. `file:./vendor/fijii` resolves against `dist/index.js`, so it must be
// built before the package is usable. Runs on postinstall; skips quietly when
// the submodule isn't present.
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const sdk = join(here, "..", "vendor", "fijii");

if (!existsSync(join(sdk, "package.json"))) {
  console.error("[postinstall] vendor/fijii missing — run: git submodule update --init --recursive");
  process.exit(0); // don't break install when the submodule is absent
}

if (existsSync(join(sdk, "dist", "index.js"))) {
  console.error("[postinstall] vendor/fijii already built");
  process.exit(0);
}

console.error("[postinstall] building vendor/fijii …");
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

for (const args of [["ci"], ["run", "build"]]) {
  const r = spawnSync(npm, args, { cwd: sdk, stdio: "inherit" });
  if (r.status !== 0) {
    console.error(`[postinstall] vendor/fijii: npm ${args.join(" ")} failed`);
    process.exit(r.status ?? 1);
  }
}
