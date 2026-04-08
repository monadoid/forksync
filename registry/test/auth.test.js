import assert from "node:assert/strict";
import test from "node:test";
import { parseBearerToken, resolveGithubIdentity, verifyGithubSourceOwnership } from "../src/auth.js";

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
    return new Response(JSON.stringify({ login: "monadoid", id: 42, name: "Mona" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  });

  assert.deepEqual(identity, {
    login: "monadoid",
    id: "42",
    name: "Mona",
  });
});

test("verifyGithubSourceOwnership rejects private repositories", async () => {
  const result = await verifyGithubSourceOwnership(
    "token",
    "monadoid/forksync",
    "main",
    async (url) => {
      if (String(url).endsWith("/repos/monadoid/forksync")) {
        return new Response(
          JSON.stringify({
            private: true,
            permissions: { admin: true, push: true },
          }),
          { status: 200, headers: { "content-type": "application/json" } }
        );
      }
      throw new Error(`unexpected url ${url}`);
    }
  );

  assert.equal(result.ok, false);
  assert.equal(result.status, 400);
});

test("verifyGithubSourceOwnership accepts writable public repos with existing branches", async () => {
  const result = await verifyGithubSourceOwnership(
    "token",
    "https://github.com/monadoid/forksync.git",
    "release/v1",
    async (url) => {
      if (String(url).endsWith("/repos/monadoid/forksync")) {
        return new Response(
          JSON.stringify({
            private: false,
            stargazers_count: 7,
            forks_count: 2,
            pushed_at: "2026-04-08T00:00:00Z",
            permissions: { admin: false, push: true },
          }),
          { status: 200, headers: { "content-type": "application/json" } }
        );
      }
      if (String(url).endsWith("/repos/monadoid/forksync/branches/release%2Fv1")) {
        return new Response(JSON.stringify({ name: "release/v1" }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }
      throw new Error(`unexpected url ${url}`);
    }
  );

  assert.equal(result.ok, true);
  assert.equal(result.repo, "monadoid/forksync");
});
