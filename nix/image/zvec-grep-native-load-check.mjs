import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";

const base = process.argv[2];
const require = createRequire(`${base}/package.json`);

for (const name of [
  "@zvec/zvec",
  "sharp",
  "@huggingface/transformers",
  "onnxruntime-node",
]) {
  require(name);
  console.log(`loaded ${name}`);
}

const { rgPath } = await import(`${base}/node_modules/@vscode/ripgrep/lib/index.js`);
console.log(execFileSync(rgPath, ["--version"]).toString().trim());
