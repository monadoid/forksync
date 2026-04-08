function rowToSource(row) {
  return {
    id: row.id,
    upstream_repo: row.upstream_repo,
    source_repo: row.source_repo,
    tracked_branch: row.tracked_branch,
    display_name: row.display_name,
    summary: row.summary,
    visibility: row.visibility,
    stars: row.stars,
    forks: row.forks,
    last_pushed_at: row.last_pushed_at,
    verified: Boolean(row.verified),
    verified_by_login: row.verified_by_login,
    published_by_login: row.published_by_login,
    published_at: row.published_at,
    updated_at: row.updated_at,
    metadata: safeParseJson(row.metadata_json),
  };
}

function safeParseJson(text) {
  if (!text) {
    return {};
  }

  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}

export async function listSources(db, query = "") {
  const sql = query.trim()
    ? `
      SELECT *
      FROM sources
      WHERE visibility = 'public'
        AND verified = 1
        AND (
          lower(display_name) LIKE lower(?)
          OR lower(source_repo) LIKE lower(?)
          OR lower(upstream_repo) LIKE lower(?)
          OR lower(summary) LIKE lower(?)
        )
      ORDER BY stars DESC, forks DESC, updated_at DESC
      LIMIT 50
    `
    : `
      SELECT *
      FROM sources
      WHERE visibility = 'public'
        AND verified = 1
      ORDER BY stars DESC, forks DESC, updated_at DESC
      LIMIT 50
    `;
  const params = query.trim() ? Array.from({ length: 4 }, () => `%${query.trim()}%`) : [];
  const result = await db.prepare(sql).bind(...params).all();
  return (result.results ?? []).map(rowToSource);
}

export async function getSourcesByIds(db, ids) {
  if (!ids.length) {
    return [];
  }

  const placeholders = ids.map(() => "?").join(", ");
  const result = await db
    .prepare(
      `
      SELECT *
      FROM sources
      WHERE id IN (${placeholders})
        AND visibility = 'public'
      ORDER BY stars DESC, forks DESC, updated_at DESC
    `
    )
    .bind(...ids)
    .all();

  return (result.results ?? []).map(rowToSource);
}

export async function upsertSource(db, source, identity) {
  const now = new Date().toISOString();
  const record = {
    id: source.id,
    upstream_repo: source.upstream_repo,
    source_repo: source.source_repo,
    tracked_branch: source.tracked_branch,
    display_name: source.display_name,
    summary: source.summary ?? "",
    visibility: source.visibility ?? "public",
    stars: Number(source.stars ?? 0),
    forks: Number(source.forks ?? 0),
    last_pushed_at: source.last_pushed_at ?? null,
    verified: source.verified ? 1 : 0,
    verified_by_login: source.verified_by_login ?? identity.login,
    published_by_login: identity.login,
    published_at: source.published_at ?? now,
    updated_at: now,
    metadata_json: JSON.stringify(source.metadata ?? {}),
  };

  await db
    .prepare(
      `
      INSERT INTO sources (
        id, upstream_repo, source_repo, tracked_branch, display_name, summary,
        visibility, stars, forks, last_pushed_at, verified, verified_by_login,
        published_by_login, published_at, updated_at, metadata_json
      ) VALUES (
        ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
      )
      ON CONFLICT(id) DO UPDATE SET
        upstream_repo = excluded.upstream_repo,
        source_repo = excluded.source_repo,
        tracked_branch = excluded.tracked_branch,
        display_name = excluded.display_name,
        summary = excluded.summary,
        visibility = excluded.visibility,
        stars = excluded.stars,
        forks = excluded.forks,
        last_pushed_at = excluded.last_pushed_at,
        verified = excluded.verified,
        verified_by_login = excluded.verified_by_login,
        published_by_login = excluded.published_by_login,
        updated_at = excluded.updated_at,
        metadata_json = excluded.metadata_json
    `
    )
    .bind(
      record.id,
      record.upstream_repo,
      record.source_repo,
      record.tracked_branch,
      record.display_name,
      record.summary,
      record.visibility,
      record.stars,
      record.forks,
      record.last_pushed_at,
      record.verified,
      record.verified_by_login,
      record.published_by_login,
      record.published_at,
      record.updated_at,
      record.metadata_json
    )
    .run();

  return record;
}

export async function unpublishSource(db, sourceId, identity) {
  const now = new Date().toISOString();
  await db
    .prepare(
      `
      UPDATE sources
      SET visibility = 'private',
          updated_at = ?,
          published_by_login = ?
      WHERE id = ?
    `
    )
    .bind(now, identity.login, sourceId)
    .run();

  return { id: sourceId, visibility: "private", updated_at: now };
}
