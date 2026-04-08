import { integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const sources = sqliteTable("sources", {
  id: text("id").primaryKey(),
  upstreamRepo: text("upstream_repo").notNull(),
  sourceRepo: text("source_repo").notNull(),
  trackedBranch: text("tracked_branch").notNull(),
  displayName: text("display_name").notNull(),
  summary: text("summary").notNull(),
  visibility: text("visibility").notNull(),
  stars: integer("stars").notNull(),
  forks: integer("forks").notNull(),
  lastPushedAt: text("last_pushed_at"),
  verified: integer("verified").notNull(),
  verifiedByLogin: text("verified_by_login"),
  publishedByLogin: text("published_by_login").notNull(),
  publishedAt: text("published_at").notNull(),
  updatedAt: text("updated_at").notNull(),
  metadataJson: text("metadata_json").notNull(),
});
