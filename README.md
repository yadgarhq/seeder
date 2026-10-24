# seeder

The D35 seeder: the one writer of org-class project rows, declared in GitOps
(ADR-0705, ADR-0934). Plan: `yadgarhq/docs` `plans/the-d35-seeder.md`.

## What it does

One Argo-synced Job run is one sync (ADR-0940, no CronJob):

1. Read the declaration the chart rendered into `ConfigMap/seeder-registry`
   from the adopter's `registry` values, and check it with the store's rules.
2. Check that the mounted client leaf is the seeder's (`seeder-caller`).
3. Dial `project` over mutual TLS and send the whole declaration as ONE
   `ProjectService.SyncProjects` (ADR-0936). `project` checks the caller and
   relays to `project-db`, which applies it in one transaction.
4. Log the five counts (`inserted`, `updated`, `archived`, `reactivated`,
   `unchanged`) and exit 0. Any refusal exits 1 with the code and the
   message `project` relays. Only `UNAVAILABLE` is retried.

The seeder does not archive anything itself. The store (`project-db`) applies
the sync in its one transaction:

- It inserts a declared row that has no row yet.
- It updates a declared row whose `displayName` or `sourceRepo` differs.
- It reactivates a declared row that is archived.
- It archives every active org-class row that the declaration no longer names
  (ADR-0934, ADR-0938). Archive is a status flip. The row is never deleted.

The mass-archive guard (ADR-0939) protects against a truncated declaration.
With `allowMassArchive: false`, the store refuses a sync that would archive
strictly more than half of the active org-class rows. Roots count. Exactly
half is allowed. A cold install never trips the guard. The refusal is
`FAILED_PRECONDITION` and it writes nothing. The seeder logs the code and the
message, which names both counts and the field, and exits 1. Re-declare the
rows, or set `allowMassArchive: true` for a deliberate large cleanup.

## Team access (plans/team-access.md)

When `registry.teams` is declared, the run has one more step before step 3:

1. Dial `iam` over mutual TLS and send the declared teams and members as ONE
   `IamService.SyncTeams` (ADR-0963 (1)). The seeder presents the same client
   leaf, `seeder-client-tls`, as it presents to `project`.
2. Send `SyncProjects` to `project` only after `SyncTeams` succeeded. A
   refused `SyncTeams` ends the run with exit 1 and no `SyncProjects`: new
   grants beside the old members can admit a caller that neither declaration
   admits.
3. After both calls, exit 1 if `project` echoes a `grants_declared` that differs
   from the number sent (it predates grants), or if `iam` lists an unknown
   member, or if it lists a pending administrator while `administrators` has a
   live member. A pending administrator with no live member is the first sync of
   a cold install (flow F2): exit 0, and the log says the bootstrap
   `create-user` is expected next.

With `registry.teams` absent or empty, no `grants` key may appear, the seeder
makes no `SyncTeams` call, and the run is what it was before team access. The
`iam.*` chart values and the `IAM_*` variables exist only when teams are
declared. `iam.tls.caSecret` has no default.

It never dials `project-db` or `iam-db` (ADR-0936, ADR-0938). It serves no RPC.

## Identity (ADR-0942)

- mTLS client leaf: `seeder-client-tls`, common name `seeder-caller`, issued
  by `platform`.
- `scope.user_id`: the fixed system principal `yadgar:system:seeder`.
- `scope.request_id`: minted by the seeder once per sync, the same on every
  retry attempt.

## Configuration

Every knob is an environment variable with no compiled-in default (ADR-0569 as
amended by ADR-0705). The chart renders all of them.

| Variable                          | Chart key                       |
| --------------------------------- | ------------------------------- |
| `SEEDER_REGISTRY_FILE`            | the `registry` block (mounted)  |
| `PROJECT_HOST`, `PROJECT_PORT`    | `project.host`, `project.port`  |
| `PROJECT_REQUEST_TIMEOUT_SECONDS` | `project.requestTimeoutSeconds` |
| `PROJECT_TLS_CA_FILE`             | `project.tls.caSecret`          |
| `PROJECT_TLS_CLIENT_CERT_FILE`    | `project.tls.clientCertSecret`  |
| `PROJECT_TLS_CLIENT_KEY_FILE`     | `project.tls.clientCertSecret`  |
| `PROJECT_TLS_DOMAIN` (optional)   | `project.tls.domain`            |
| `SEEDER_SYNC_ATTEMPTS`            | `sync.attempts`                 |
| `SEEDER_SYNC_BACKOFF_SECONDS`     | `sync.backoffSeconds`           |

## The declaration

```yaml
registry:
  roots: # single-segment namespace anchors (ADR-0673)
    - path: yadgarhq
      sourceRepo: yadgarhq/argocd
  projects: # each strictly under a root; [] is legal
    - path: yadgarhq/gateway
      displayName: gateway
      sourceRepo: yadgarhq/gateway
  allowMassArchive: false # required, no default (ADR-0939)
```

The `registry` block is required, with no default (ADR-0857 contract step).
A render without it refuses and names the key; through the parent chart the
key is `seeder.registry`. Releases before v0.2.0 rendered nothing without it.

## Checks

`cargo test`, `pytest scripts/tests/` (needs helm), `pre-commit run --all-files`.
