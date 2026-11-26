//! The GitLab GraphQL documents IsItGreen can run.
//!
//! As for GitHub (`github_queries.rs`), the frontend never sends query text
//! for GitLab. It names one of the operations below and passes variables;
//! project cards send a list of projects, and [`projects_query`] builds the
//! batched document from the constant pieces in this file. Unlike GitHub's,
//! every operation's variables are checked against a fixed shape before they
//! are sent. A compromised webview can therefore only run these read-only
//! queries, whatever the token could do. Fields are limited to ones GitLab
//! Free (CE) has, so self-hosted instances work too.

use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::gitlab::is_valid_login;

/// Projects per card request; matches `PROJECTS_PER_REQUEST` in
/// `src/providers/gitlab/queries.ts`. GitLab caps each request's query
/// complexity at 250, and a project with every widget costs about 75.
pub const MAX_PROJECTS_PER_REQUEST: usize = 2;
/// Upper bound on any list the frontend asks for.
const MAX_LIST: u32 = 100;
/// Longest project path accepted, in bytes.
const MAX_PATH: usize = 500;
/// Longest label accepted, in bytes.
const MAX_NAME: usize = 255;
/// Longest branch name accepted, in bytes. The name comes from GitLab's own
/// answer (`rootRef`), and Git sets no short limit, so this is generous.
const MAX_REF: usize = 1024;
/// Most digits in a merge request or issue IID.
const MAX_IID_DIGITS: usize = 20;

macro_rules! mr_fields {
    () => {
        concat!(
            "iid title webUrl draft updatedAt approved detailedMergeStatus conflicts ",
            "reference(full: true) ",
            "author { username } ",
            "headPipeline { status } ",
            "approvedBy(first: 1) { nodes { username } }"
        )
    };
}

macro_rules! issue_fields {
    () => {
        concat!(
            "iid title webUrl updatedAt ",
            "reference(full: true) ",
            "author { username } ",
            "labels(first: 4) { nodes { title color } }"
        )
    };
}

/// The default branch's tree, whose last commit is the branch head. That
/// commit's newest pipeline can belong to another ref (a merge request, a
/// branch at the same commit) or be a scan that does not count towards the
/// branch's status, so a few are fetched and the frontend chooses. The 10
/// must match `PIPELINES_FETCHED` in `src/providers/gitlab/queries.ts`.
macro_rules! ci_tree {
    () => {
        "tree { lastCommit { pipelines(first: 10) { nodes { status ref source } } } }"
    };
}

/// The default branch's own pipelines on its head commit, for when the
/// project query's unfiltered page held none of them.
const BRANCH_PIPELINE: &str = concat!(
    "query IsItGreenBranchPipeline($path: ID!, $ref: String!, $n: Int!) { ",
    "project(fullPath: $path) { ",
    "repository { tree { lastCommit { pipelines(ref: $ref, first: $n) { nodes { status ref source } } } } } ",
    "} }"
);

/// Merge requests that need the viewer; assigned issues follow in
/// [`ASSIGNED_ISSUES`]. Review requests the viewer already answered
/// (approved, reviewed, requested changes) are left out, as on GitLab's own
/// merge request dashboard; one whose approval was reset counts again.
const INBOX: &str = concat!(
    "query IsItGreenInbox(",
    "$n: Int!, $withReview: Boolean!, $withMine: Boolean!, $withAssigned: Boolean!",
    ") { currentUser { username ",
    "reviewRequestedMergeRequests(",
    "state: opened, reviewStates: [UNREVIEWED, REVIEW_STARTED, UNAPPROVED], first: $n, sort: UPDATED_DESC",
    ") @include(if: $withReview) { count nodes { ",
    mr_fields!(),
    " } } ",
    "authoredMergeRequests(state: opened, first: $n, sort: UPDATED_DESC) @include(if: $withMine) { ",
    "count nodes { ",
    mr_fields!(),
    " } } ",
    "assignedMergeRequests(state: opened, first: $n, sort: UPDATED_DESC) @include(if: $withAssigned) { ",
    "count nodes { ",
    issue_fields!(),
    " } } } }"
);

const ASSIGNED_ISSUES: &str = concat!(
    "query IsItGreenAssignedIssues($me: String!, $n: Int!) { ",
    "issues(assigneeUsernames: [$me], state: opened, first: $n, sort: UPDATED_DESC) { ",
    "count nodes { ",
    issue_fields!(),
    " } } }"
);

/// Projects the viewer is a member of, most recently active first. GitLab
/// answers a rejected token as an anonymous request, so the viewer tells an
/// expired token from no projects.
const PROJECT_LIST: &str = concat!(
    "query IsItGreenProjectList($n: Int!) { ",
    "viewer: currentUser { username } ",
    "projects(membership: true, first: $n, sort: \"latest_activity_desc\") { ",
    "nodes { fullPath archived lastActivityAt } } }"
);

macro_rules! details_common {
    () => {
        concat!(
            "createdAt description userNotesCount ",
            "labels(first: 10) { nodes { title color } } ",
            "assignees(first: 10) { nodes { username } }"
        )
    };
}

/// One merge request or issue of a project, for the details view.
const DETAILS: &str = concat!(
    "query IsItGreenDetails($path: ID!, $iid: String!, $n: Int!, $mr: Boolean!) { ",
    "project(fullPath: $path) { ",
    "mergeRequest(iid: $iid) @include(if: $mr) { ",
    details_common!(),
    " sourceBranch targetBranch ",
    "diffStatsSummary { additions deletions fileCount } ",
    "diffStats { additions deletions } ",
    "headPipeline { jobs(first: $n, retried: false) { count nodes { name status allowFailure webPath } } } ",
    "reviewers(first: $n) { nodes { username mergeRequestInteraction { reviewState } } } ",
    "approvedBy(first: $n) { nodes { username } } } ",
    "issue(iid: $iid) @skip(if: $mr) { ",
    details_common!(),
    " } } }"
);

/// What one project card needs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSpec {
    path: String,
    #[serde(default)]
    ci: bool,
    /// How many open merge requests to fetch, if the card shows them.
    #[serde(default)]
    merge_requests: Option<u32>,
    #[serde(default)]
    issues: Option<IssuesSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuesSpec {
    limit: u32,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BranchPipelineVars {
    path: String,
    #[serde(rename = "ref")]
    git_ref: String,
    n: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InboxVars {
    n: u32,
    // The flags are only checked to be booleans; GitLab reads them.
    #[expect(dead_code)]
    with_review: bool,
    #[expect(dead_code)]
    with_mine: bool,
    #[expect(dead_code)]
    with_assigned: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssignedIssuesVars {
    me: String,
    n: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectListVars {
    n: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailsVars {
    path: String,
    iid: String,
    n: u32,
    // Only checked to be a boolean; GitLab reads it.
    #[expect(dead_code)]
    mr: bool,
}

fn ensure(ok: bool) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::ForbiddenOperation)
    }
}

fn list_size(n: u32) -> Result<u32> {
    ensure((1..=MAX_LIST).contains(&n))?;
    Ok(n)
}

fn text(s: &str, max: usize) -> Result<&str> {
    ensure((1..=max).contains(&s.len()))?;
    Ok(s)
}

fn parse<T: DeserializeOwned>(variables: &Value) -> Result<T> {
    T::deserialize(variables).map_err(|_| Error::ForbiddenOperation)
}

/// Builds the batched project query. Only the constant pieces above and
/// numbered variable names end up in the text; every value from the
/// frontend is passed as a variable. Results are under the aliases `p0`,
/// `p1`, ... in the order given, next to the signed-in user as `viewer`.
pub fn projects_query(projects: &[ProjectSpec]) -> Result<(String, Value)> {
    ensure(!projects.is_empty() && projects.len() <= MAX_PROJECTS_PER_REQUEST)?;
    let mut defs: Vec<String> = Vec::new();
    let mut selections: Vec<String> = Vec::new();
    let mut vars = Map::new();

    for (i, project) in projects.iter().enumerate() {
        defs.push(format!("$path{i}: ID!"));
        vars.insert(
            format!("path{i}"),
            Value::from(text(&project.path, MAX_PATH)?),
        );

        let mut parts = vec!["fullPath webUrl visibility archived".to_owned()];
        parts.push(if project.ci {
            concat!("repository { rootRef ", ci_tree!(), " }").to_owned()
        } else {
            "repository { rootRef }".to_owned()
        });

        if let Some(n) = project.merge_requests {
            defs.push(format!("$mr{i}: Int!"));
            vars.insert(format!("mr{i}"), Value::from(list_size(n)?));
            parts.push(format!(
                "mergeRequests(state: opened, first: $mr{i}, sort: UPDATED_DESC) {{ count nodes {{ {} }} }}",
                mr_fields!()
            ));
        }
        if let Some(issues) = &project.issues {
            defs.push(format!("$is{i}: Int!"));
            vars.insert(format!("is{i}"), Value::from(list_size(issues.limit)?));
            let mut label_arg = String::new();
            if let Some(label) = &issues.label {
                defs.push(format!("$il{i}: [String!]"));
                vars.insert(format!("il{i}"), Value::from(vec![text(label, MAX_NAME)?]));
                label_arg = format!(", labelName: $il{i}");
            }
            parts.push(format!(
                "issues(state: opened, first: $is{i}{label_arg}, sort: UPDATED_DESC) {{ count nodes {{ {} }} }}",
                issue_fields!()
            ));
        }
        selections.push(format!(
            "p{i}: project(fullPath: $path{i}) {{ {} }}",
            parts.join(" ")
        ));
    }

    // GitLab answers a request with a bad token as if it were anonymous, so
    // private projects would look missing; the viewer tells the two apart.
    let query = format!(
        "query IsItGreenProjects({}) {{ viewer: currentUser {{ username }} {} }}",
        defs.join(", "),
        selections.join(" ")
    );
    Ok((query, Value::Object(vars)))
}

/// Resolves what the frontend asked for into a document and its variables.
/// `IsItGreenProjects` takes `{ "projects": [ProjectSpec, ...] }`; the fixed
/// operations take their own variables, which must have exactly the expected
/// keys and types and values in range.
pub fn resolve(operation: &str, variables: Value) -> Result<(String, Value)> {
    ensure(variables.is_object())?;
    let document = match operation {
        "IsItGreenProjects" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Args {
                projects: Vec<ProjectSpec>,
            }
            let args: Args = parse(&variables)?;
            return projects_query(&args.projects);
        }
        "IsItGreenBranchPipeline" => {
            let vars: BranchPipelineVars = parse(&variables)?;
            text(&vars.path, MAX_PATH)?;
            text(&vars.git_ref, MAX_REF)?;
            list_size(vars.n)?;
            BRANCH_PIPELINE
        }
        "IsItGreenInbox" => {
            let vars: InboxVars = parse(&variables)?;
            list_size(vars.n)?;
            INBOX
        }
        "IsItGreenAssignedIssues" => {
            let vars: AssignedIssuesVars = parse(&variables)?;
            ensure(is_valid_login(&vars.me))?;
            list_size(vars.n)?;
            ASSIGNED_ISSUES
        }
        "IsItGreenProjectList" => {
            let vars: ProjectListVars = parse(&variables)?;
            list_size(vars.n)?;
            PROJECT_LIST
        }
        "IsItGreenDetails" => {
            let vars: DetailsVars = parse(&variables)?;
            text(&vars.path, MAX_PATH)?;
            ensure(
                (1..=MAX_IID_DIGITS).contains(&vars.iid.len())
                    && vars.iid.bytes().all(|b| b.is_ascii_digit()),
            )?;
            list_size(vars.n)?;
            DETAILS
        }
        _ => return Err(Error::ForbiddenOperation),
    };
    Ok((document.to_owned(), variables))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql_guard::ensure_read_only;
    use serde_json::json;

    /// The variables `src/providers/gitlab/index.ts` sends, with the same
    /// literal values as the matching expectations in `gitlab.test.ts`.
    fn frontend_requests() -> Vec<(&'static str, Value)> {
        vec![
            (
                "IsItGreenProjects",
                json!({ "projects": [{
                    "path": "acme/platform/api", "ci": true, "mergeRequests": 50,
                    "issues": { "limit": 3, "label": "bug" },
                }] }),
            ),
            (
                "IsItGreenProjects",
                json!({ "projects": [
                    { "path": "acme/platform/api", "ci": true, "mergeRequests": null, "issues": null },
                    { "path": "acme/platform/api", "ci": false, "mergeRequests": null,
                      "issues": { "limit": 1, "label": null } },
                ] }),
            ),
            (
                "IsItGreenBranchPipeline",
                json!({ "path": "acme/platform/p0", "ref": "main", "n": 10 }),
            ),
            (
                "IsItGreenInbox",
                json!({ "n": 5, "withReview": true, "withMine": true, "withAssigned": true }),
            ),
            (
                "IsItGreenAssignedIssues",
                json!({ "me": "jane.doe", "n": 5 }),
            ),
            ("IsItGreenProjectList", json!({ "n": 100 })),
            (
                "IsItGreenDetails",
                json!({ "path": "acme/platform/api", "iid": "4", "n": 50, "mr": false }),
            ),
            (
                "IsItGreenDetails",
                json!({ "path": "acme/platform/api", "iid": "12", "n": 50, "mr": true }),
            ),
        ]
    }

    fn refused(operation: &str, variables: Value) {
        assert!(
            matches!(
                resolve(operation, variables.clone()),
                Err(Error::ForbiddenOperation)
            ),
            "{operation} {variables}"
        );
    }

    fn accepted(operation: &str, variables: Value) -> (String, Value) {
        resolve(operation, variables.clone())
            .unwrap_or_else(|e| panic!("{operation} {variables}: {e:?}"))
    }

    /// Collapses whitespace, so text checks do not depend on line breaks.
    fn flat(doc: &str) -> String {
        doc.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// `base` with `key` set to `value`.
    fn with(base: &Value, key: &str, value: Value) -> Value {
        let mut v = base.clone();
        v[key] = value;
        v
    }

    fn without(base: &Value, key: &str) -> Value {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove(key);
        v
    }

    fn projects(spec: Value) -> Value {
        json!({ "projects": [spec] })
    }

    #[test]
    fn frontend_requests_resolve_to_named_read_only_documents() {
        for (operation, variables) in frontend_requests() {
            let (doc, _) = accepted(operation, variables);
            ensure_read_only(&doc).unwrap();
            assert!(
                doc.trim_start().starts_with(&format!("query {operation}(")),
                "{operation}: {doc}"
            );
            assert_eq!(
                doc.matches('{').count(),
                doc.matches('}').count(),
                "{operation}"
            );
        }
    }

    #[test]
    fn unknown_operations_and_query_text_are_refused() {
        for op in [
            "mutation { destroyProject(input: {}) { errors } }",
            "query IsItGreenInbox($n: Int!) { currentUser { username } }",
            "{ currentUser { username } }",
            "IsItGreenRepos",
            "isitgreeninbox",
            "IsItGreenInbox ",
            "",
        ] {
            refused(op, json!({ "n": 5 }));
        }
        for variables in [json!([]), json!(null), json!("n=5"), json!(5)] {
            refused("IsItGreenProjectList", variables.clone());
            refused("IsItGreenProjects", variables);
        }
    }

    #[test]
    fn unknown_variable_keys_are_refused() {
        refused(
            "IsItGreenProjectList",
            json!({ "n": 5, "query": "mutation" }),
        );
        refused(
            "IsItGreenInbox",
            json!({ "n": 5, "withReview": true, "withMine": true, "withAssigned": true, "x": 1 }),
        );
        refused(
            "IsItGreenAssignedIssues",
            json!({ "me": "a", "n": 5, "nn": 5 }),
        );
        let spec = json!({ "path": "a/b", "ci": true });
        accepted("IsItGreenProjects", projects(spec.clone()));
        refused(
            "IsItGreenProjects",
            json!({ "projects": [spec], "extra": 1 }),
        );
        refused(
            "IsItGreenProjects",
            projects(with(&spec, "query", json!("mutation"))),
        );
        refused(
            "IsItGreenProjects",
            projects(with(
                &spec,
                "issues",
                json!({ "limit": 1, "labels": ["x"] }),
            )),
        );
        // camelCase only: the Rust field name is not an alias.
        refused(
            "IsItGreenProjects",
            projects(with(&spec, "merge_requests", json!(5))),
        );
    }

    #[test]
    fn missing_required_variables_are_refused() {
        refused("IsItGreenProjectList", json!({}));
        refused("IsItGreenProjects", json!({}));
        refused("IsItGreenProjects", projects(json!({ "ci": true })));
        let inbox = json!({ "n": 5, "withReview": true, "withMine": true, "withAssigned": true });
        for key in ["n", "withReview", "withMine", "withAssigned"] {
            refused("IsItGreenInbox", without(&inbox, key));
        }
        let details = json!({ "path": "a/b", "iid": "4", "n": 50, "mr": false });
        for key in ["path", "iid", "n", "mr"] {
            refused("IsItGreenDetails", without(&details, key));
        }
    }

    #[test]
    fn wrong_types_are_refused() {
        // One wrong type per field type, and at least one per operation.
        for (operation, valid, key, bad) in [
            ("IsItGreenProjectList", json!({ "n": 5 }), "n", json!("5")),
            (
                "IsItGreenInbox",
                json!({ "n": 5, "withReview": true, "withMine": true, "withAssigned": true }),
                "withReview",
                json!("true"),
            ),
            (
                "IsItGreenDetails",
                json!({ "path": "a/b", "iid": "4", "n": 50, "mr": false }),
                "iid",
                json!(4),
            ),
            (
                "IsItGreenBranchPipeline",
                json!({ "path": "a/b", "ref": "main", "n": 10 }),
                "ref",
                json!(["main"]),
            ),
            (
                "IsItGreenAssignedIssues",
                json!({ "me": "a", "n": 5 }),
                "me",
                json!(["a"]),
            ),
        ] {
            accepted(operation, valid.clone());
            refused(operation, with(&valid, key, bad));
        }

        let spec = json!({ "path": "a/b" });
        accepted("IsItGreenProjects", projects(spec.clone()));
        for (key, bad) in [("mergeRequests", json!("50")), ("issues", json!(3))] {
            refused("IsItGreenProjects", projects(with(&spec, key, bad)));
        }
        refused("IsItGreenProjects", json!({ "projects": "a/b" }));
    }

    /// Every request that carries a list size, with that size set to `n`.
    fn sized_requests(n: i64) -> Vec<(&'static str, Value)> {
        vec![
            ("IsItGreenProjectList", json!({ "n": n })),
            (
                "IsItGreenBranchPipeline",
                json!({ "path": "a/b", "ref": "main", "n": n }),
            ),
            (
                "IsItGreenInbox",
                json!({ "n": n, "withReview": true, "withMine": false, "withAssigned": false }),
            ),
            ("IsItGreenAssignedIssues", json!({ "me": "a", "n": n })),
            (
                "IsItGreenDetails",
                json!({ "path": "a/b", "iid": "1", "n": n, "mr": true }),
            ),
            (
                "IsItGreenProjects",
                projects(json!({ "path": "a/b", "mergeRequests": n })),
            ),
            (
                "IsItGreenProjects",
                projects(json!({ "path": "a/b", "issues": { "limit": n } })),
            ),
        ]
    }

    #[test]
    fn list_sizes_must_be_between_1_and_100() {
        for n in [1, 100] {
            for (operation, variables) in sized_requests(n) {
                accepted(operation, variables);
            }
        }
        for n in [0, 101, -1, 1 << 32] {
            for (operation, variables) in sized_requests(n) {
                refused(operation, variables);
            }
        }
    }

    #[test]
    fn strings_are_bounded() {
        let pipeline = |path: &str, r#ref: &str| json!({ "path": path, "ref": r#ref, "n": 10 });
        let details = |path: &str| json!({ "path": path, "iid": "1", "n": 10, "mr": false });
        let path_ok = ["a".repeat(500), "é".repeat(250)];
        // 501 bytes either way; "é" is two bytes in UTF-8.
        let path_long = ["a".repeat(501), format!("a{}", "é".repeat(250))];
        for path in &path_ok {
            accepted("IsItGreenBranchPipeline", pipeline(path, "main"));
            accepted("IsItGreenDetails", details(path));
            accepted("IsItGreenProjects", projects(json!({ "path": path })));
        }
        for path in path_long.iter().map(String::as_str).chain([""]) {
            refused("IsItGreenBranchPipeline", pipeline(path, "main"));
            refused("IsItGreenDetails", details(path));
            refused("IsItGreenProjects", projects(json!({ "path": path })));
        }

        accepted(
            "IsItGreenBranchPipeline",
            pipeline("a/b", &"r".repeat(1024)),
        );
        refused(
            "IsItGreenBranchPipeline",
            pipeline("a/b", &"r".repeat(1025)),
        );
        refused("IsItGreenBranchPipeline", pipeline("a/b", ""));

        let label = |label: &str| {
            projects(json!({ "path": "a/b", "issues": { "limit": 5, "label": label } }))
        };
        accepted("IsItGreenProjects", label(&"l".repeat(255)));
        refused("IsItGreenProjects", label(&"l".repeat(256)));
        refused("IsItGreenProjects", label(""));
    }

    #[test]
    fn iid_is_a_short_decimal_string() {
        let details = |iid: Value| json!({ "path": "a/b", "iid": iid, "n": 10, "mr": true });
        accepted("IsItGreenDetails", details(json!("1")));
        accepted("IsItGreenDetails", details(json!("9".repeat(20))));
        for bad in [
            json!("9".repeat(21)),
            json!(""),
            json!("4a"),
            json!("-4"),
            json!(" 4"),
            json!("4\n"),
            json!("\u{0664}"), // ARABIC-INDIC DIGIT FOUR
            json!(4),
        ] {
            refused("IsItGreenDetails", details(bad));
        }
    }

    #[test]
    fn assigned_issues_need_a_valid_username() {
        accepted(
            "IsItGreenAssignedIssues",
            json!({ "me": "a_b.c-d", "n": 5 }),
        );
        refused(
            "IsItGreenAssignedIssues",
            json!({ "me": "x\") { id } mutation {", "n": 5 }),
        );
    }

    #[test]
    fn hostile_paths_and_labels_only_travel_as_variables() {
        let evil_path = "x\") { id } mutation { destroyProject";
        let evil_label = "\"}) { id } mutation {";
        let shape = |path: &str, label: &str| {
            projects(json!({
                "path": path, "ci": true, "mergeRequests": 50,
                "issues": { "limit": 3, "label": label },
            }))
        };
        let (plain, _) = accepted("IsItGreenProjects", shape("a/b", "bug"));
        let (query, vars) = accepted("IsItGreenProjects", shape(evil_path, evil_label));
        assert_eq!(query, plain);
        ensure_read_only(&query).unwrap();
        assert_eq!(vars["path0"], evil_path);
        assert_eq!(vars["il0"], json!([evil_label]));
    }

    #[test]
    fn projects_query_has_the_viewer_and_numbered_aliases() {
        let (query, vars) = accepted(
            "IsItGreenProjects",
            json!({ "projects": [
                { "path": "a/b", "ci": true, "mergeRequests": 50,
                  "issues": { "limit": 3, "label": "bug" } },
                { "path": "c/d", "issues": { "limit": 2 } },
            ] }),
        );
        let query = flat(&query);
        assert!(query.starts_with("query IsItGreenProjects("), "{query}");
        assert!(
            query.contains("viewer: currentUser { username }"),
            "{query}"
        );
        assert!(query.contains("p0: project(fullPath: $path0)"), "{query}");
        assert!(query.contains("p1: project(fullPath: $path1)"), "{query}");
        assert_eq!(
            vars,
            json!({ "path0": "a/b", "mr0": 50, "is0": 3, "il0": ["bug"], "path1": "c/d", "is1": 2 })
        );
    }

    #[test]
    fn projects_query_only_requests_enabled_widgets() {
        let (query, vars) = accepted(
            "IsItGreenProjects",
            projects(json!({ "path": "a/b", "ci": true })),
        );
        let query = flat(&query);
        assert!(
            query.contains("pipelines(first: 10) { nodes { status ref source } }"),
            "{query}"
        );
        assert!(!query.contains("mergeRequests("), "{query}");
        assert!(!query.contains("issues("), "{query}");
        assert_eq!(vars, json!({ "path0": "a/b" }));

        // Widgets off, written both as null and left out.
        for spec in [
            json!({ "path": "a/b" }),
            json!({ "path": "a/b", "ci": false, "mergeRequests": null, "issues": null }),
        ] {
            let (query, vars) = accepted("IsItGreenProjects", projects(spec));
            assert!(!query.contains("pipelines"), "{query}");
            assert!(!query.contains("mergeRequests("), "{query}");
            assert!(!query.contains("issues("), "{query}");
            assert_eq!(vars, json!({ "path0": "a/b" }));
        }

        // An issues widget without a label passes no label filter.
        for issues in [json!({ "limit": 4 }), json!({ "limit": 4, "label": null })] {
            let (query, vars) = accepted(
                "IsItGreenProjects",
                projects(json!({ "path": "a/b", "issues": issues })),
            );
            assert!(query.contains("issues("), "{query}");
            assert!(!query.contains("labelName"), "{query}");
            assert!(!query.contains("$il0"), "{query}");
            assert_eq!(vars, json!({ "path0": "a/b", "is0": 4 }));
        }
    }

    #[test]
    fn at_most_two_projects_per_request() {
        let one = || json!({ "path": "a/b", "ci": true });
        // `PROJECTS_PER_REQUEST` in src/providers/gitlab/index.ts sends two.
        accepted("IsItGreenProjects", json!({ "projects": [one(), one()] }));
        refused(
            "IsItGreenProjects",
            json!({ "projects": [one(), one(), one()] }),
        );
        refused("IsItGreenProjects", json!({ "projects": [] }));
    }

    #[test]
    fn inbox_leaves_out_reviews_already_given() {
        let (doc, _) = accepted(
            "IsItGreenInbox",
            json!({ "n": 5, "withReview": true, "withMine": true, "withAssigned": true }),
        );
        assert!(
            flat(&doc).contains("reviewStates: [UNREVIEWED, REVIEW_STARTED, UNAPPROVED]"),
            "{doc}"
        );
    }
}
