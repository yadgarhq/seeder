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
4. Log the five counts and exit 0. Any refusal exits 1 with the code and the
   message `project` relays. Only `UNAVAILABLE` is retried.

It never dials `project-db` (ADR-0936, ADR-0938). It serves no RPC.

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

With no `registry` block the chart renders nothing (ADR-0857 expand step).

## Checks

`cargo test`, `pytest scripts/tests/` (needs helm), `pre-commit run --all-files`.
