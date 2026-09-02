// Runs the Khronos glTF-Validator over one file and prints its report as JSON.
//
// The npm package is the only channel that works here: there is no Homebrew
// formula, the published binaries are x64, and our CI runner is arm64. This
// package is Dart compiled to JS, so it is architecture independent. Bun runs
// it, and `node:` imports are part of the runtime both share.
//
// `check/validator.rs` reads what this prints. Issue limits are left at the
// default, which is unlimited.
//
// Exit codes, because a broken asset and a broken tool are different things:
//   0  a report is on stdout
//   3  the file is not glTF at all, reason on stderr, tool version on stdout
//   2  this script was called wrongly
//   anything else  the tool itself is broken
import { readFile } from "node:fs/promises";

import { validateBytes, version } from "gltf-validator";

const [path] = process.argv.slice(2);
if (!path) {
  console.error("usage: validate.mjs <file.glb>");
  process.exit(2);
}

try {
  const report = await validateBytes(new Uint8Array(await readFile(path)), {
    uri: path,
  });
  process.stdout.write(JSON.stringify(report));
} catch (error) {
  // A rejection means the input is not glTF at all. Report it as one line,
  // not as an uncaught exception with a runtime stack trace. There is no
  // report to carry the version, so stdout carries it instead: every finding
  // names the tool that measured it.
  process.stdout.write(version());
  console.error(`${path}: ${error}`);
  process.exit(3);
}
