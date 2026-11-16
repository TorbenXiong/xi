import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

test("desktop build rejects unknown options and preserves existing staging files", async () => {
  const directory = await mkdtemp(join(tmpdir(), "xi desktop build 测试 "));
  try {
    await mkdir(join(directory, "scripts"));
    const script = join(directory, "scripts", "build-desktop.mjs");
    await copyFile(new URL("../../scripts/build-desktop.mjs", import.meta.url), script);
    const cli = join(directory, "app", "node_modules", "@tauri-apps", "cli");
    await mkdir(cli, { recursive: true });
    await writeFile(join(cli, "tauri.js"), "throw new Error('must not run');");
    const staging = join(directory, "src-tauri", "runtime");
    await mkdir(staging, { recursive: true });
    const runtime = join(staging, process.platform === "win32" ? "xi-agent-cli.exe" : "xi-agent-cli");
    await writeFile(runtime, "existing Runtime fixture");

    for (const [option, message] of [["--unexpected", "仅支持 --no-bundle 参数"], ["--no-bundle", "Runtime 暂存文件已存在"]]) {
      const result = spawnSync(process.execPath, [script, option], { cwd: directory, encoding: "utf8", windowsHide: true });
      assert.ifError(result.error);
      assert.equal(result.status, 1);
      assert.ok(result.stderr.includes(message), result.stderr);
      assert.equal(await readFile(runtime, "utf8"), "existing Runtime fixture");
    }
  } finally {
    await rm(directory, { recursive: true });
  }
});
