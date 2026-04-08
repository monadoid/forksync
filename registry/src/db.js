import { and, desc, eq, inArray, like, or } from "drizzle-orm";
import { drizzle } from "drizzle-orm/d1";
import { sources } from "./schema.js";

function rowToSource(row) {
  return {
    id: row.id,
    upstream_repo: row.upstreamRepo ?? row.upstream_repo,
    source_repo: row.sourceRepo ?? row.source_repo,
    tracked_branch: row.trackedBranch ?? row.tracked_branch,
    display_name: row.displayName ?? row.display_name,
    summary: row.summary,
    visibility: row.visibility,
    stars: row.stars,
    forks: row.forks,
    last_pushed_at: row.lastPushedAt ?? row.last_pushed_at,
    verified: Boolean(row.verified),
    verified_by_login: row.verifiedByLogin ?? row.verified_by_login,
    published_by_login: row.publishedByLogin ?? row.published_by_login,
    published_at: row.publishedAt ?? row.published_at,
    updated_at: row.updatedAt ?? row.updated_at,
    metadata: safeParseJson(row.metadataJson ?? row.metadata_json),
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
  const orm = drizzle(db);
  const normalized = query.trim();
  const filters = [eq(sources.visibility, "public"), eq(sources.verified, 1)];
  if (normalized) {
    const pattern = `%${normalized}%`;
    filters.push(
      or(
        like(sources.displayName, pattern),
        like(sources.sourceRepo, pattern),
        like(sources.upstreamRepo, pattern),
        like(sources.summary, pattern)
      )
    );
  }

  const rows = await orm
    .select()
    .from(sources)
    .where(and(...filters))
    .orderBy(desc(sources.stars), desc(sources.forks), desc(sources.updatedAt))
    .limit(50);
  return rows.map(rowToSource);
}

export async function getSourcesByIds(db, ids) {
  if (!ids.length) {
    return [];
  }

  const orm = drizzle(db);
  const rows = await orm
    .select()
    .from(sources)
    .where(and(inArray(sources.id, ids), eq(sources.visibility, "public")))
    .orderBy(desc(sources.stars), desc(sources.forks), desc(sources.updatedAt));
  return rows.map(rowToSource);
}

export async function getSourceById(db, id) {
  const orm = drizzle(db);
  const rows = await orm.select().from(sources).where(eq(sources.id, id)).limit(1);
  return rows.length ? rowToSource(rows[0]) : null;
}

export async function upsertSource(db, source, identity) {
  const orm = drizzle(db);
  const now = new Date().toISOString();
  const record = {
    id: source.id,
    upstreamRepo: source.upstream_repo,
    sourceRepo: source.source_repo,
    trackedBranch: source.tracked_branch,
    displayName: source.display_name,
    summary: source.summary ?? "",
    visibility: source.visibility ?? "public",
    stars: Number(source.stars ?? 0),
    forks: Number(source.forks ?? 0),
    lastPushedAt: source.last_pushed_at ?? null,
    verified: source.verified ? 1 : 0,
    verifiedByLogin: source.verified_by_login ?? identity.login,
    publishedByLogin: identity.login,
    publishedAt: source.published_at ?? now,
    updatedAt: now,
    metadataJson: JSON.stringify(source.metadata ?? {}),
  };

  await orm.insert(sources).values(record).onConflictDoUpdate({
    target: sources.id,
    set: {
      upstreamRepo: record.upstreamRepo,
      sourceRepo: record.sourceRepo,
      trackedBranch: record.trackedBranch,
      displayName: record.displayName,
      summary: record.summary,
      visibility: record.visibility,
      stars: record.stars,
      forks: record.forks,
      lastPushedAt: record.lastPushedAt,
      verified: record.verified,
      verifiedByLogin: record.verifiedByLogin,
      publishedByLogin: record.publishedByLogin,
      updatedAt: record.updatedAt,
      metadataJson: record.metadataJson,
    },
  });

  return rowToSource(record);
}

export async function unpublishSource(db, sourceId, identity) {
  const orm = drizzle(db);
  const now = new Date().toISOString();
  await orm
    .update(sources)
    .set({
      visibility: "private",
      updatedAt: now,
      publishedByLogin: identity.login,
    })
    .where(eq(sources.id, sourceId));

  return { id: sourceId, visibility: "private", updated_at: now };
}
