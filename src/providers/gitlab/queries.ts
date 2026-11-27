/**
 * The GitLab queries IsItGreen runs, by name.
 *
 * As for GitHub, the query documents live in the backend
 * (`src-tauri/src/gitlab_queries.rs`): the frontend can only name one of them
 * and pass variables, so it can never send a query of its own. Project cards
 * send a list of projects instead, and the backend builds the batched query
 * from fixed pieces.
 */

import type { PinnedRepo } from '../../config/schema';

export type GitLabOperation =
  | 'IsItGreenProjects'
  | 'IsItGreenBranchPipeline'
  | 'IsItGreenInbox'
  | 'IsItGreenAssignedIssues'
  | 'IsItGreenProjectList'
  | 'IsItGreenDetails';

/** Pinned project cards, batched; variables from `buildProjectsRequest`. */
export const PROJECTS_QUERY: GitLabOperation = 'IsItGreenProjects';
/**
 * The default branch's own pipelines on its head commit, for when the
 * projects query's unfiltered page held none of them.
 */
export const BRANCH_PIPELINE_QUERY: GitLabOperation = 'IsItGreenBranchPipeline';
/**
 * Merge requests that need the viewer; assigned issues follow in
 * `ASSIGNED_ISSUES_QUERY`. Review requests the viewer already answered are
 * left out, as on GitLab's own merge request dashboard.
 */
export const INBOX_QUERY: GitLabOperation = 'IsItGreenInbox';
export const ASSIGNED_ISSUES_QUERY: GitLabOperation = 'IsItGreenAssignedIssues';
/** Projects the viewer is a member of, most recently active first. */
export const PROJECT_LIST_QUERY: GitLabOperation = 'IsItGreenProjectList';
/** One merge request or issue of a project, for the details view. */
export const DETAILS_QUERY: GitLabOperation = 'IsItGreenDetails';

/** Open merge requests fetched per project; see `OPEN_PRS_FETCHED` for GitHub. */
export const OPEN_MRS_FETCHED = 50;

/**
 * GitLab caps each request's query complexity at 250. A project with every
 * widget costs about 75, so two per request leaves room to spare. Must match
 * `MAX_PROJECTS_PER_REQUEST` in `src-tauri/src/gitlab_queries.rs`, which
 * refuses more.
 */
export const PROJECTS_PER_REQUEST = 2;

/** Pipelines of the default branch's head commit searched for the branch's own. */
export const PIPELINES_FETCHED = 10;

/** Where the backend puts the signed-in user in the projects and project-list queries. */
export const VIEWER_ALIAS = 'viewer';

/** Where the backend puts the result for the `index`th project. */
export function projectAlias(index: number): string {
  return `p${index}`;
}

/** What one project card needs, as the backend's `ProjectSpec` expects it. */
export interface ProjectSpec {
  path: string;
  ci: boolean;
  /** Pipelines to fetch; set exactly when `ci` is. */
  pipelines: number | null;
  mergeRequests: number | null;
  issues: { limit: number; label: string | null } | null;
}

/** Variables for `IsItGreenProjects`: one spec per project, in order. */
export function buildProjectsRequest(repos: readonly PinnedRepo[]): { projects: ProjectSpec[] } {
  return {
    projects: repos.map((repo) => {
      const issues = repo.widgets.find((w) => w.type === 'issues');
      const ci = repo.widgets.some((w) => w.type === 'ci');
      return {
        path: `${repo.owner}/${repo.name}`,
        ci,
        pipelines: ci ? PIPELINES_FETCHED : null,
        mergeRequests: repo.widgets.some((w) => w.type === 'pullRequests') ? OPEN_MRS_FETCHED : null,
        issues: issues ? { limit: issues.limit, label: issues.label || null } : null,
      };
    }),
  };
}

/** Projects offered as suggestions when pinning. */
export const PROJECT_SUGGESTIONS_FETCHED = 100;

/** Most jobs and reviewers fetched for the details view. */
export const DETAILS_LIST_FETCHED = 50;
