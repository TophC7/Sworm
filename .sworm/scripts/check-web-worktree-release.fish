#!/usr/bin/env fish
# Run against an existing disposable daemon; exercises real WebSocket ownership and Git deletion.
if test (count $argv) -ne 1
    echo 'Usage: fish .sworm/scripts/check-web-worktree-release.fish http://127.0.0.1:<daemon-port>' >&2
    exit 2
end

set repo (path resolve (path dirname (status filename))/../..)
set fixture (mktemp -d /tmp/sworm-release-contract-XXXXXX)
function cleanup --on-event fish_exit --inherit-variable fixture
    command rm -rf -- $fixture
end

command git init --quiet $fixture/main; or exit 1
command git -C $fixture/main -c user.name=Sworm -c user.email=sworm@example.invalid commit --quiet --allow-empty -m Initial; or exit 1
set -lx SWORM_RELEASE_ORIGIN $argv[1]
set -lx SWORM_RELEASE_FIXTURE $fixture
set -lx SWORM_RELEASE_TRANSPORT $repo/src/lib/api/transport.web.ts

bun --eval '
import assert from "node:assert/strict";
import { stat } from "node:fs/promises";
const { createWebHostTransport } = await import(process.env.SWORM_RELEASE_TRANSPORT);
const workbenchId = `release-contract-${crypto.randomUUID()}`;
const main = `${process.env.SWORM_RELEASE_FIXTURE}/main`;
const linked = `${process.env.SWORM_RELEASE_FIXTURE}/linked`;
// Use Buns real WebSocket with the same Origin header a browser supplies.
const NativeWebSocket = globalThis.WebSocket;
globalThis.WebSocket = class extends NativeWebSocket {
  constructor(url) { super(url, { headers: { Origin: new URL(process.env.SWORM_RELEASE_ORIGIN).origin } }); }
};
const host = createWebHostTransport({
  origin: process.env.SWORM_RELEASE_ORIGIN,
  workbenchId,
  onConnectionState(state, error) { if (error) console.error(error); },
  onReconnected() { throw new Error("Regression connection unexpectedly reconnected"); },
  onLspDisconnected() { throw new Error("Regression never starts an LSP server"); }
});
const remove = () => host.call("git_worktree", { path: main, op: { kind: "remove", worktreePath: linked, force: false } });
let ready = false;
let expired = false;
const deadline = setTimeout(() => { expired = true; host.dispose(); }, 10_000);
try {
  await host.ready;
  ready = true;
  await host.call("git_worktree", { path: main, op: { kind: "add", destination: linked, checkout: { kind: "new_branch", name: "release-contract", base: "HEAD" } } });
  // This owning path read bypasses FolderIntent registration: release still must reach the daemon.
  await host.call("git_get_summary", { path: linked });
  await host.call("folder_release", { folderPath: linked });
  await host.call("folder_release", { folderPath: linked });
  assert.equal((await remove()).kind, "removed");

  await host.call("git_worktree", { path: main, op: { kind: "add", destination: linked, checkout: { kind: "existing_branch", name: "release-contract" } } });
  await host.call("folder_claim", { folderPath: linked });
  const firstRelease = host.call("folder_release", { folderPath: linked });
  const repeatedRelease = host.call("folder_release", { folderPath: linked });
  const laterClaim = host.call("folder_claim", { folderPath: linked });
  await Promise.all([firstRelease, repeatedRelease, laterClaim]);
  await assert.rejects(remove(), /still open|close.*window/i);
  assert.equal((await stat(linked)).isDirectory(), true);
  await host.call("folder_release", { folderPath: linked });
  assert.equal((await remove()).kind, "removed");
  console.log("Real web release regression passed: undeclared ownership released; repeated queued releases preserve a later claim.");
} catch (error) {
  if (expired) throw new Error("Web release regression exceeded its 10-second connection/operation deadline", { cause: error });
  throw error;
} finally {
  try {
    if (ready && !expired) await host.call("workbench_close", { id: workbenchId });
  } finally {
    host.dispose();
    clearTimeout(deadline);
    globalThis.WebSocket = NativeWebSocket;
  }
}
'; or exit 1
