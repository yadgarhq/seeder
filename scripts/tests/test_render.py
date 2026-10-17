"""What the chart renders, and every refusal it makes (plans/the-d35-seeder.md
stage 5, tests 6 to 8).

EACH REFUSAL CARRIES ITS POISON INPUT (ADR-0606): the case renders a values
file that breaks exactly one rule and asserts the refusal names the key. A
refusal for the wrong reason does not pass, because the needle is the key or
the path, not "Error".

HELM IS REQUIRED and the suite fails without it, naming helm (ADR-0650).
`ci-pr.yaml`'s precommit job installs it before pre-commit runs.

Run: python3 -m pytest scripts/tests/ -q
"""

from __future__ import annotations

import copy
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest
import yaml

CHART = Path(__file__).resolve().parents[2] / "chart"

TLS = {"caSecret": "project-tls", "clientCertSecret": "seeder-client-tls"}

REGISTRY = {
    "roots": [{"path": "yadgarhq", "sourceRepo": "yadgarhq/argocd"}],
    "projects": [
        {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway"},
        {"path": "yadgarhq/project-db", "sourceRepo": "yadgarhq/project-db"},
    ],
    "allowMassArchive": False,
}


def render(values: dict | None) -> subprocess.CompletedProcess[str]:
    helm = shutil.which("helm")
    assert helm, "helm is not on PATH; this suite renders the chart and cannot run without it"
    with tempfile.NamedTemporaryFile("w", suffix=".yaml", delete=False) as f:
        yaml.safe_dump(values or {}, f)
        path = f.name
    try:
        return subprocess.run(
            [helm, "template", "t", str(CHART), "-f", path],
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        Path(path).unlink()


def values(registry: dict | None = None, **overrides: object) -> dict:
    out: dict = {"project": {"tls": dict(TLS)}}
    if registry is not None:
        out["registry"] = registry
    out.update(overrides)
    return out


def objects(stdout: str) -> list[dict]:
    return [o for o in yaml.safe_load_all(stdout) if o]


def rendered(v: dict) -> dict[str, dict]:
    result = render(v)
    assert result.returncode == 0, result.stderr
    return {o["kind"]: o for o in objects(result.stdout)}


def refuses(v: dict, needle: str) -> None:
    result = render(v)
    assert result.returncode != 0, f"must refuse naming {needle!r}; rendered:\n{result.stdout}"
    assert needle in result.stderr, f"the refusal must name {needle!r}:\n{result.stderr}"


def poisoned(mutate) -> dict:  # noqa: ANN001
    r = copy.deepcopy(REGISTRY)
    mutate(r)
    return values(r)


# --- test 6: two projects render a ConfigMap carrying both, and the Job -----


def test_two_projects_render_a_configmap_carrying_both_and_a_job_mounting_it() -> None:
    kinds = rendered(values(REGISTRY))
    assert set(kinds) == {"ServiceAccount", "ConfigMap", "Job"}

    cm = kinds["ConfigMap"]
    assert cm["metadata"]["name"] == "seeder-registry"
    assert cm["metadata"]["annotations"]["argocd.argoproj.io/sync-wave"] == "1"
    doc = json.loads(cm["data"]["registry.json"])
    assert doc == {
        "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}],
        "projects": [
            {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway"},
            {"path": "yadgarhq/project-db", "displayName": "", "sourceRepo": "yadgarhq/project-db"},
        ],
        "allowMassArchive": False,
    }

    job = kinds["Job"]
    annotations = job["metadata"]["annotations"]
    assert annotations["argocd.argoproj.io/sync-wave"] == "1"
    assert annotations["argocd.argoproj.io/sync-options"] == "Replace=true,Force=true"
    assert "ttlSecondsAfterFinished" not in job["spec"]
    pod = job["spec"]["template"]["spec"]
    assert pod["restartPolicy"] == "Never"
    assert pod["automountServiceAccountToken"] is False
    # `project`'s ingress policy selects `app: <client>`; the estate adds `seeder`.
    assert job["spec"]["template"]["metadata"]["labels"]["app"] == "seeder"
    (container,) = pod["containers"]
    assert container["args"] == ["sync-projects"]
    mounts = {m["name"]: m["mountPath"] for m in container["volumeMounts"]}
    assert mounts["registry"] == "/etc/seeder"
    volumes = {v["name"]: v for v in pod["volumes"]}
    assert volumes["registry"]["configMap"]["name"] == "seeder-registry"
    env = {e["name"]: e["value"] for e in container["env"]}
    assert env["SEEDER_REGISTRY_FILE"] == "/etc/seeder/registry.json"


def test_the_job_dials_project_on_50052_and_names_no_db() -> None:
    job = rendered(values(REGISTRY))["Job"]
    env = {e["name"]: e["value"] for e in job["spec"]["template"]["spec"]["containers"][0]["env"]}
    assert env["PROJECT_HOST"] == "project"
    # 50052 is project's port; 50051 is project-db's (ledger 890).
    assert env["PROJECT_PORT"] == "50052"
    assert not [k for k in env if "DB" in k], env


def test_the_job_mounts_the_seeder_s_leaf_and_project_s_authority() -> None:
    pod = rendered(values(REGISTRY))["Job"]["spec"]["template"]["spec"]
    volumes = {v["name"]: v for v in pod["volumes"]}
    assert volumes["client-cert"]["secret"]["secretName"] == "seeder-client-tls"
    assert volumes["project-ca"]["secret"]["secretName"] == "project-tls"
    env = {e["name"]: e["value"] for e in pod["containers"][0]["env"]}
    assert env["PROJECT_TLS_CLIENT_CERT_FILE"] == "/var/run/secrets/client-cert/client.crt"
    assert env["PROJECT_TLS_CA_FILE"] == "/var/run/config/project-ca/ca.pem"


def test_every_binary_knob_is_rendered() -> None:
    """The binary has no compiled-in default (ADR-0705), so the chart renders
    every required variable it reads."""
    env = {
        e["name"]
        for e in rendered(values(REGISTRY))["Job"]["spec"]["template"]["spec"]["containers"][0]["env"]
    }
    assert {
        "SEEDER_REGISTRY_FILE",
        "PROJECT_HOST",
        "PROJECT_PORT",
        "PROJECT_REQUEST_TIMEOUT_SECONDS",
        "PROJECT_TLS_CA_FILE",
        "PROJECT_TLS_CLIENT_CERT_FILE",
        "PROJECT_TLS_CLIENT_KEY_FILE",
        "SEEDER_SYNC_ATTEMPTS",
        "SEEDER_SYNC_BACKOFF_SECONDS",
    } <= env


def test_ledger_1244_option_b_renders() -> None:
    r = copy.deepcopy(REGISTRY)
    r["roots"].append({"path": "estate", "sourceRepo": "yadgarhq/estate"})
    r["projects"].append({"path": "estate/smoke", "sourceRepo": "yadgarhq/estate"})
    doc = json.loads(rendered(values(r))["ConfigMap"]["data"]["registry.json"])
    assert [x["path"] for x in doc["roots"]] == ["yadgarhq", "estate"]
    assert "estate/smoke" in [x["path"] for x in doc["projects"]]


def test_an_empty_project_list_is_legal() -> None:
    r = copy.deepcopy(REGISTRY)
    r["projects"] = []
    assert json.loads(rendered(values(r))["ConfigMap"]["data"]["registry.json"])["projects"] == []


def test_mass_archive_true_is_carried() -> None:
    r = copy.deepcopy(REGISTRY)
    r["allowMassArchive"] = True
    assert json.loads(rendered(values(r))["ConfigMap"]["data"]["registry.json"])["allowMassArchive"]


# --- the expand step: no registry, nothing rendered ------------------------


def test_no_registry_renders_nothing() -> None:
    result = render({})
    assert result.returncode == 0, result.stderr
    assert objects(result.stdout) == []


def test_an_empty_registry_block_is_refused_not_skipped() -> None:
    refuses(values({}), "registry.allowMassArchive")


# --- test 7: every declaration refusal, each with its poison ---------------


@pytest.mark.parametrize(
    ("mutate", "needle"),
    [
        (lambda r: r["roots"].__setitem__(0, {"path": "yadgarhq/two", "sourceRepo": "x/y"}),
         "registry.roots[0].path \"yadgarhq/two\" has more than one segment"),
        (lambda r: r["projects"].append({"path": "other/thing", "sourceRepo": "x/y"}),
         "registry.projects[2].path \"other/thing\" is not under a declared root"),
        (lambda r: r["projects"].append({"path": "gateway", "sourceRepo": "x/y"}),
         "registry.projects[2].path \"gateway\" is not under a declared root"),
        (lambda r: r["projects"].append({"path": "local/max/x", "sourceRepo": "x/y"}),
         "registry.projects[2].path \"local/max/x\" is under the reserved segment"),
        (lambda r: r["roots"].append({"path": "LOCAL", "sourceRepo": "x/y"}),
         "registry.roots[1].path \"LOCAL\" is under the reserved segment"),
        (lambda r: r["projects"].append({"path": "yadgarhq/GATEWAY", "sourceRepo": "x/y"}),
         "registry.projects[2].path \"yadgarhq/GATEWAY\" is declared twice"),
        (lambda r: r["projects"][0].__setitem__("sourceRepo", ""),
         "registry.projects[0].sourceRepo is empty"),
        (lambda r: r["projects"][0].pop("sourceRepo"),
         "registry.projects[0].sourceRepo is empty"),
        (lambda r: r["projects"].append({"path": "yadgarhq/has space", "sourceRepo": "x/y"}),
         "contains the segment \"has space\""),
        (lambda r: r["projects"].append({"path": "yadgarhq//x", "sourceRepo": "x/y"}),
         "registry.projects[2].path \"yadgarhq//x\" contains the segment \"\""),
        (lambda r: r["projects"].append({"path": "yadgarhq/..", "sourceRepo": "x/y"}),
         "contains the segment \"..\""),
        (lambda r: r["projects"][0].__setitem__("displayName", "x" * 256),
         "registry.projects[0].displayName"),
        (lambda r: r.__setitem__("roots", []),
         "registry.roots must name at least one root"),
        (lambda r: r.pop("roots"),
         "registry.roots must be a list"),
        (lambda r: r.pop("allowMassArchive"),
         "registry.allowMassArchive must be set to true or false"),
        (lambda r: r.__setitem__("allowMassArchive", "false"),
         "registry.allowMassArchive must be set to true or false"),
    ],
)
def test_a_bad_declaration_is_refused_naming_the_key(mutate, needle: str) -> None:  # noqa: ANN001
    refuses(poisoned(mutate), needle)


def test_a_rendered_registry_over_the_size_guard_is_refused() -> None:
    r = copy.deepcopy(REGISTRY)
    # Distinct paths, each well inside the store's limits, together over the guard.
    r["projects"] = [
        {"path": f"yadgarhq/p{i:05d}", "displayName": "d" * 200, "sourceRepo": "yadgarhq/argocd"}
        for i in range(3000)
    ]
    refuses(values(r), "the guard is 786432")


def test_an_unknown_key_in_a_row_is_refused_by_the_schema() -> None:
    r = copy.deepcopy(REGISTRY)
    r["projects"][0]["sourcerepo"] = "typo"
    refuses(values(r), "sourcerepo")


# --- test 8: the TLS knobs absent refuse (ADR-0854) -------------------------


@pytest.mark.parametrize("key", ["caSecret", "clientCertSecret"])
def test_an_absent_tls_secret_refuses(key: str) -> None:
    v = values(REGISTRY)
    del v["project"]["tls"][key]
    refuses(v, f"project.tls.{key}")


def test_a_deadline_shorter_than_the_retry_loop_refuses() -> None:
    v = values(REGISTRY, job={"activeDeadlineSeconds": 100, "backoffLimit": 2})
    refuses(v, "job.activeDeadlineSeconds (100) is shorter than the retry loop")


def test_a_request_timeout_outside_the_dial_range_refuses() -> None:
    v = values(REGISTRY)
    v["project"]["requestTimeoutSeconds"] = 60
    refuses(v, "requestTimeoutSeconds")
