# Repository settings

Settings that live on GitHub rather than in the code. Keep this page in sync when they change.

## Protecting `main`
Merging into `main` is blocked until CI is fully green. CI's last job, `ci-ok`
([ci.yml](../../.github/workflows/ci.yml)), succeeds only when every other job succeeded: lint, tests, MSRV, docs,
dependency audit, both package builds and every distribution test. The ruleset requires that single check, so it never
needs updating when distributions are added to or removed from the matrix.

**Status:** active since 2026-11-23 (ruleset `main`, id 27909093). Checked through the API
(`GET /repos/mhsiddiqui/housekeeper/rules/branches/main`): deletion, non-fast-forward, pull request and required
status check `ci-ok` (GitHub Actions, strict) rules apply to `main`.

### Ruleset (Settings → Rules → Rulesets → New ruleset → New branch ruleset)
| Field | Value |
|---|---|
| Ruleset name | `main` |
| Enforcement status | Active |
| Bypass list | empty (nobody, including admins, can merge a red pull request) |
| Target branches | Include default branch |
| Restrict deletions | on |
| Block force pushes | on |
| Require a pull request before merging | on; required approvals `0` (single maintainer) |
| Require status checks to pass | on; **Require branches to be up to date before merging** on; add check `ci-ok` (source: GitHub Actions) |

`ci-ok` appears in the check picker after it has run at least once, i.e. after the first pull request that contains it.

### Same ruleset from the command line
With the [GitHub CLI](https://cli.github.com) signed in as a repository admin:

```sh
gh api --method POST repos/mhsiddiqui/housekeeper/rulesets --input - <<'EOF'
{
  "name": "main",
  "target": "branch",
  "enforcement": "active",
  "bypass_actors": [],
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "pull_request", "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false } },
    { "type": "required_status_checks", "parameters": {
        "strict_required_status_checks_policy": true,
        "required_status_checks": [ { "context": "ci-ok", "integration_id": 15368 } ] } }
  ]
}
EOF
```
`15368` is the GitHub Actions app, so only a check reported by Actions can satisfy `ci-ok`.
