import { requireGithubIdentity, verifyGithubSourceOwnership } from "./auth.js";
import { getSourceById, getSourcesByIds, listSources, upsertSource, unpublishSource } from "./db.js";
import { renderBootstrapCommand } from "./command.js";
import { renderHtmlPage } from "./render.js";

function json(data, init = {}) {
  return new Response(JSON.stringify(data, null, 2), {
    headers: { "content-type": "application/json; charset=utf-8" },
    ...init,
  });
}

async function readJson(request) {
  const text = await request.text();
  return text ? JSON.parse(text) : {};
}

function buildSourceRecord(body) {
  return {
    id: body.id ?? body.source_id ?? crypto.randomUUID(),
    upstream_repo: body.upstream_repo,
    source_repo: body.source_repo,
    tracked_branch: body.tracked_branch,
    display_name: body.display_name ?? body.source_repo,
    summary: body.summary ?? "",
    visibility: body.visibility ?? "public",
    stars: body.stars ?? 0,
    forks: body.forks ?? 0,
    last_pushed_at: body.last_pushed_at ?? null,
    verified: Boolean(body.verified ?? true),
    verified_by_login: body.verified_by_login ?? null,
    metadata: body.metadata ?? {},
  };
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/") {
      const sources = await listSources(env.REGISTRY_DB, url.searchParams.get("query") ?? "");
      return new Response(renderHtmlPage(sources, url.searchParams.get("query") ?? ""), {
        headers: { "content-type": "text/html; charset=utf-8" },
      });
    }

    if (request.method === "GET" && url.pathname === "/api/sources") {
      const query = url.searchParams.get("query") ?? "";
      return json({ sources: await listSources(env.REGISTRY_DB, query) });
    }

    if (request.method === "POST" && url.pathname === "/api/bootstrap-command") {
      const body = await readJson(request);
      const sources = await getSourcesByIds(env.REGISTRY_DB, Array.isArray(body.source_ids) ? body.source_ids : []);
      return json({
        sources,
        command: renderBootstrapCommand(sources),
      });
    }

    if (request.method === "POST" && url.pathname === "/api/publish") {
      const auth = await requireGithubIdentity(request);
      if (auth.error) {
        return auth.error;
      }

      const body = await readJson(request);
      const source = buildSourceRecord(body);
      const verified = await verifyGithubSourceOwnership(auth.token, source.source_repo, source.tracked_branch);
      if (!verified.ok) {
        return json({ error: verified.reason }, { status: verified.status });
      }
      source.source_repo = verified.repo;
      source.stars = Number(verified.repoPayload.stargazers_count ?? source.stars ?? 0);
      source.forks = Number(verified.repoPayload.forks_count ?? source.forks ?? 0);
      source.last_pushed_at = verified.repoPayload.pushed_at ?? source.last_pushed_at ?? null;
      source.verified = true;
      source.verified_by_login = auth.identity.login;
      const record = await upsertSource(env.REGISTRY_DB, source, auth.identity);
      return json({ source: record }, { status: 201 });
    }

    if (request.method === "POST" && url.pathname === "/api/update") {
      const auth = await requireGithubIdentity(request);
      if (auth.error) {
        return auth.error;
      }

      const body = await readJson(request);
      const source = buildSourceRecord(body);
      const verified = await verifyGithubSourceOwnership(auth.token, source.source_repo, source.tracked_branch);
      if (!verified.ok) {
        return json({ error: verified.reason }, { status: verified.status });
      }
      source.source_repo = verified.repo;
      source.stars = Number(verified.repoPayload.stargazers_count ?? source.stars ?? 0);
      source.forks = Number(verified.repoPayload.forks_count ?? source.forks ?? 0);
      source.last_pushed_at = verified.repoPayload.pushed_at ?? source.last_pushed_at ?? null;
      source.verified = true;
      source.verified_by_login = auth.identity.login;
      const record = await upsertSource(env.REGISTRY_DB, source, auth.identity);
      return json({ source: record });
    }

    if (request.method === "POST" && url.pathname === "/api/unpublish") {
      const auth = await requireGithubIdentity(request);
      if (auth.error) {
        return auth.error;
      }

      const body = await readJson(request);
      if (!body.source_id) {
        return new Response("source_id is required", { status: 400 });
      }
      const existing = await getSourceById(env.REGISTRY_DB, body.source_id);
      if (!existing) {
        return new Response("source_id was not found", { status: 404 });
      }
      const verified = await verifyGithubSourceOwnership(auth.token, existing.source_repo, existing.tracked_branch);
      if (!verified.ok) {
        return json({ error: verified.reason }, { status: verified.status });
      }

      const result = await unpublishSource(env.REGISTRY_DB, body.source_id, auth.identity);
      return json({ source: result });
    }

    return new Response("not found", { status: 404 });
  },
};
