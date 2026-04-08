const GITHUB_API_BASE = "https://api.github.com";

export function parseBearerToken(request) {
  const header = request.headers.get("authorization") ?? request.headers.get("Authorization");
  if (!header) {
    return null;
  }

  const match = header.match(/^Bearer\s+(.+)$/i);
  return match ? match[1].trim() : null;
}

export async function resolveGithubIdentity(token, fetchImpl = fetch) {
  if (!token) {
    return null;
  }

  const response = await fetchImpl(`${GITHUB_API_BASE}/user`, {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "ForkSync Registry",
    },
  });

  if (!response.ok) {
    return null;
  }

  const payload = await response.json();
  if (!payload || typeof payload.login !== "string") {
    return null;
  }

  return {
    login: payload.login,
    id: String(payload.id ?? ""),
    name: typeof payload.name === "string" ? payload.name : null,
  };
}

export async function requireGithubIdentity(request, fetchImpl = fetch) {
  const token = parseBearerToken(request);
  if (!token) {
    return { error: new Response("missing GitHub bearer token", { status: 401 }) };
  }

  const identity = await resolveGithubIdentity(token, fetchImpl);
  if (!identity) {
    return { error: new Response("invalid GitHub bearer token", { status: 401 }) };
  }

  return { token, identity };
}

function normalizeRepoName(repo) {
  if (typeof repo !== "string") {
    return null;
  }
  const trimmed = repo.trim().replace(/^https?:\/\/github\.com\//i, "").replace(/\.git$/i, "");
  if (!/^[^/\s]+\/[^/\s]+$/.test(trimmed)) {
    return null;
  }
  return trimmed;
}

async function githubJson(url, token, fetchImpl = fetch) {
  const response = await fetchImpl(url, {
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "ForkSync Registry",
    },
  });
  if (!response.ok) {
    return null;
  }
  return response.json();
}

export async function verifyGithubSourceOwnership(token, sourceRepo, trackedBranch, fetchImpl = fetch) {
  const repo = normalizeRepoName(sourceRepo);
  if (!repo) {
    return { ok: false, status: 400, reason: "source_repo must be an owner/repo GitHub repository" };
  }

  const repoPayload = await githubJson(`${GITHUB_API_BASE}/repos/${repo}`, token, fetchImpl);
  if (!repoPayload) {
    return { ok: false, status: 404, reason: "source repo was not found via GitHub API" };
  }
  if (repoPayload.private) {
    return { ok: false, status: 400, reason: "only public repositories can be published to the public registry" };
  }
  const canAdmin = Boolean(repoPayload.permissions?.admin);
  const canPush = Boolean(repoPayload.permissions?.push);
  if (!canAdmin && !canPush) {
    return { ok: false, status: 403, reason: "GitHub token does not have write access to the source repo" };
  }

  const encodedBranch = encodeURIComponent(trackedBranch);
  const branchPayload = await githubJson(
    `${GITHUB_API_BASE}/repos/${repo}/branches/${encodedBranch}`,
    token,
    fetchImpl
  );
  if (!branchPayload) {
    return { ok: false, status: 404, reason: "tracked branch was not found on the source repo" };
  }

  return {
    ok: true,
    repo,
    repoPayload,
    branchPayload,
  };
}
