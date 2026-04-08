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
