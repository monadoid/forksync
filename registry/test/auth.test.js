import assert from "node:assert/strict";
import test from "node:test";
import { parseBearerToken, resolveGithubIdentity } from "../src/auth.js";

test("parseBearerToken reads the authorization header", () => {
  const request = new Request("https://example.com", {
    headers: { Authorization: "Bearer abc123" },
  });

  assert.equal(parseBearerToken(request), "abc123");
});

test("resolveGithubIdentity returns null for non-OK responses", async () => {
  const identity = await resolveGithubIdentity("token", async () => new Response("nope", { status: 401 }));
  assert.equal(identity, null);
});

test("resolveGithubIdentity maps GitHub payloads into a stable identity shape", async () => {
  const identity = await resolveGithubIdentity("token", async () => {
    return new Response(JSON.stringify({ login: "samfinton", id: 42, name: "Sam" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  });

  assert.deepEqual(identity, {
    login: "samfinton",
    id: "42",
    name: "Sam",
  });
});
