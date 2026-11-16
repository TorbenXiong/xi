import { constants, copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const executable = process.platform === "win32" ? "xi-agent-cli.exe" : "xi-agent-cli";
const builtRuntime = join(root, "target", "release", executable);
const runtimeDirectory = join(root, "src-tauri", "runtime");
const bundledRuntime = join(runtimeDirectory, executable);
const tauri = join(root, "app", "node_modules", "@tauri-apps", "cli", "tauri.js");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, CARGO_NET_OFFLINE: "true" },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} 执行失败，退出码：${result.status ?? "unknown"}`);
}

const options = process.argv.slice(2);
if (options.some((option) => option !== "--no-bundle")) throw new Error("仅支持 --no-bundle 参数");
if (!existsSync(tauri)) throw new Error(`未找到 Tauri CLI：${tauri}`);
if (existsSync(bundledRuntime)) throw new Error(`Runtime 暂存文件已存在，请先检查：${bundledRuntime}`);
run("cargo", ["build", "--release", "--offline", "--locked"]);
if (!existsSync(builtRuntime)) throw new Error(`未找到 Runtime 构建产物：${builtRuntime}`);

mkdirSync(runtimeDirectory, { recursive: true });
copyFileSync(builtRuntime, bundledRuntime, constants.COPYFILE_EXCL);
try {
  const args = ["build", "--config", join("src-tauri", "tauri.conf.json")];
  if (options.includes("--no-bundle")) args.splice(1, 0, "--no-bundle");
  run(process.execPath, [tauri, ...args]);
} finally {
  rmSync(bundledRuntime, { force: true });
}
