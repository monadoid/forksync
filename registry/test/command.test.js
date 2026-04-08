import assert from "node:assert/strict";
import test from "node:test";
import { renderBootstrapCommand } from "../src/command.js";

test("renderBootstrapCommand returns a clean init command for multiple sources", () => {
  const command = renderBootstrapCommand([
    { source_repo: "github.com/alice/widget", tracked_branch: "main" },
    { source_repo: "github.com/bob/widget", tracked_branch: "v2" },
  ]);

  assert.equal(
    command,
    "pnpx forksync init --source 'github.com/alice/widget#main' --source 'github.com/bob/widget#v2'"
  );
});

test("renderBootstrapCommand falls back to a bare init command", () => {
  assert.equal(renderBootstrapCommand([]), "pnpx forksync init");
});
