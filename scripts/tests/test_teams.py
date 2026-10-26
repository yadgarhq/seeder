"""Team access, stage 6 (plans/team-access.md §2): what the chart renders and
refuses when `registry.teams` is declared. Red numbers are the plan's.

THE SCHEMA RULE IS MEASURED, NOT ASSUMED (red 12). `administrators` present with
at least one member is refused by the draft-07 `if`/`then` + `contains` block in
`values.schema.json`, and by nothing else: `test_red_12_measurement_*` deletes the
block from a copy of the schema and shows the first two cases then render.

HELM WORDS A SCHEMA REFUSAL BY MAJOR (3.18.4 is Argo's and CI's, 4.x is a
laptop's), so each schema refusal carries one needle per major and the suite
passes on either.

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
from test_render import CHART, REGISTRY, TLS, objects, refuses, render, rendered, values

ADMIN = "0192f3c4-aaaa-7bbb-8ccc-0123456789ab"
RUNNER = "0192f3c4-bbbb-7bbb-9ccc-0123456789ab"

TEAMS = [
    {"name": "administrators", "members": [ADMIN]},
    {"name": "estate-runners", "members": [RUNNER]},
]

# helm 3.18.4 / helm 4.x, for a `contains` failure under `registry.teams`.
SCHEMA_TEAMS_REFUSALS = (
    "registry.teams: At least one of the items must match",
    "at '/registry/teams': no items match contains schema",
)


def team_registry() -> dict:
    r = copy.deepcopy(REGISTRY)
    r["roots"][0]["grants"] = [
        {"team": "administrators", "level": "readWrite"},
        {"team": "estate-runners", "level": "read"},
    ]
    r["projects"][0]["grants"] = [{"team": "estate-runners", "level": "readWrite"}]
    r["teams"] = copy.deepcopy(TEAMS)
    r["allowMassRevoke"] = False
    return r


def team_values(registry: dict | None = None, **overrides: object) -> dict:
    out = values(registry if registry is not None else team_registry())
    out["iam"] = {"tls": {"caSecret": "iam-tls"}}
    out.update(overrides)
    return out


def refuses_any(v: dict, needles: tuple[str, ...]) -> None:
    result = render(v)
    assert result.returncode != 0, f"must refuse; rendered:\n{result.stdout}"
    assert any(n in result.stderr for n in needles), (
        f"the refusal must carry one of {needles!r}:\n{result.stderr}"
    )


def doc_of(v: dict) -> dict:
    return json.loads(rendered(v)["ConfigMap"]["data"]["registry.json"])


def env_of(v: dict) -> dict[str, str]:
    job = rendered(v)["Job"]
    return {e["name"]: e["value"] for e in job["spec"]["template"]["spec"]["containers"][0]["env"]}


# --- stage 12: teams absent is refused, naming the key ----------------------


# A needle unique to the stage 12 refusal: main's older messages for the same
# inputs also say "registry.teams".
NEW = "A declaration names its teams"


def without_teams(r: dict) -> dict:
    """`r` as the transition rule allowed it: no `teams`, no `allowMassRevoke`,
    no `grants`."""
    r = copy.deepcopy(r)
    for key in ("teams", "allowMassRevoke"):
        r.pop(key, None)
    for entry in r["roots"] + r["projects"]:
        entry.pop("grants", None)
    return r


def test_stage_12_teams_absent_is_refused_naming_the_key() -> None:
    refuses(values(without_teams(REGISTRY)), NEW)


def test_stage_12_an_empty_teams_list_is_refused_naming_the_key() -> None:
    r = without_teams(REGISTRY)
    r["teams"] = []
    refuses(values(r), NEW)


def test_stage_12_a_grant_while_teams_is_absent_is_refused_naming_the_key() -> None:
    r = without_teams(REGISTRY)
    r["roots"][0]["grants"] = [{"team": "administrators", "level": "readWrite"}]
    refuses(values(r), NEW)


def test_stage_12_the_chart_ci_fixture_declares_teams() -> None:
    fixture = yaml.safe_load((CHART / "ci" / "values.yaml").read_text())
    assert fixture["registry"]["teams"], "the fixture every shared gate renders must declare teams"


# --- the rendered declaration and the Job wiring ----------------------------


def test_teams_render_into_the_declaration_and_the_job_dials_iam() -> None:
    v = team_values()
    doc = doc_of(v)
    assert doc["teams"] == TEAMS
    assert doc["allowMassRevoke"] is False
    assert doc["roots"][0]["grants"] == [
        {"team": "administrators", "level": "readWrite"},
        {"team": "estate-runners", "level": "read"},
    ]
    # Every row carries `grants` once teams are declared, empty when it declares none.
    assert doc["projects"][1]["grants"] == []

    env = env_of(v)
    assert env["IAM_HOST"] == "iam"
    # iam's module port, 50052; 50051 is iam-db's.
    assert env["IAM_PORT"] == "50052"
    assert env["IAM_REQUEST_TIMEOUT_SECONDS"] == "45"
    assert env["IAM_TLS_CA_FILE"] == "/var/run/config/iam-ca/ca.pem"
    assert not [k for k in env if "DB" in k], env

    pod = rendered(v)["Job"]["spec"]["template"]["spec"]
    volumes = {x["name"]: x for x in pod["volumes"]}
    assert volumes["iam-ca"]["secret"]["secretName"] == "iam-tls"
    # One identity to both hops: the seeder's leaf, no second one.
    assert volumes["client-cert"]["secret"]["secretName"] == TLS["clientCertSecret"]
    assert len([x for x in pod["volumes"] if x["name"].endswith("cert")]) == 1


def test_allow_mass_revoke_true_is_carried() -> None:
    r = team_registry()
    r["allowMassRevoke"] = True
    assert doc_of(team_values(r))["allowMassRevoke"] is True


def test_the_chart_ci_fixture_with_teams_added_still_renders() -> None:
    fixture = yaml.safe_load((CHART / "ci" / "values.yaml").read_text())
    fixture["registry"]["teams"] = copy.deepcopy(TEAMS)
    fixture["registry"]["allowMassRevoke"] = False
    for root in fixture["registry"]["roots"]:
        root["grants"] = [{"team": "administrators", "level": "readWrite"}]
    fixture["iam"] = {"tls": {"caSecret": "iam-tls"}}
    assert set(rendered(fixture)) == {"ServiceAccount", "ConfigMap", "Job"}


# --- red 12: the schema rule, one case each ---------------------------------


def only_teams(teams: list[dict] | None) -> dict:
    """The smallest valid declaration around `teams`: roots carry the grant
    only when `administrators` is declared, so no render check but the schema
    has a reason to refuse the three poisoned cases."""
    r = copy.deepcopy(REGISTRY)
    r.pop("teams")
    r["allowMassRevoke"] = False
    r["roots"][0].pop("grants")
    if teams is not None:
        r["teams"] = teams
    if any(t.get("name") == "administrators" for t in teams or []):
        r["roots"][0]["grants"] = [{"team": "administrators", "level": "readWrite"}]
    return team_values(r)


CASE_NO_ADMINISTRATORS = [{"name": "estate-runners", "members": [RUNNER]}]
CASE_EMPTY_MEMBERS = [{"name": "administrators", "members": []}]
CASE_ABSENT_MEMBERS = [{"name": "administrators"}]


def test_red_12_teams_without_administrators_is_refused_by_the_schema() -> None:
    refuses_any(only_teams(CASE_NO_ADMINISTRATORS), SCHEMA_TEAMS_REFUSALS)


def test_red_12_administrators_with_empty_members_is_refused_by_the_schema() -> None:
    refuses_any(only_teams(CASE_EMPTY_MEMBERS), SCHEMA_TEAMS_REFUSALS)


def test_red_12_administrators_with_members_absent_is_refused_by_the_schema() -> None:
    refuses_any(only_teams(CASE_ABSENT_MEMBERS), SCHEMA_TEAMS_REFUSALS)


def test_red_12_teams_absent_is_refused_by_the_render_check() -> None:
    """The transition rule is gone (stage 12): the schema lets an absent `teams`
    through, and `render-checks.yaml` refuses it naming the key."""
    refuses(only_teams(None), NEW)


def chart_without_the_rule(tmp: Path) -> Path:
    copy_of = tmp / "chart"
    shutil.copytree(CHART, copy_of)
    schema_path = copy_of / "values.schema.json"
    schema = json.loads(schema_path.read_text())
    registry = schema["properties"]["registry"]
    assert "if" in registry and "then" in registry, "the rule is not where this test deletes it"
    del registry["if"]
    del registry["then"]
    schema_path.write_text(json.dumps(schema))
    return copy_of


def render_in(chart: Path, v: dict) -> subprocess.CompletedProcess[str]:
    helm = shutil.which("helm")
    assert helm
    with tempfile.NamedTemporaryFile("w", suffix=".yaml", delete=False) as f:
        yaml.safe_dump(v, f)
        path = f.name
    try:
        return subprocess.run(
            [helm, "template", "t", str(chart), "-f", path],
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        Path(path).unlink()


@pytest.mark.parametrize(
    "teams",
    [CASE_NO_ADMINISTRATORS, CASE_EMPTY_MEMBERS, CASE_ABSENT_MEMBERS],
    ids=["no-administrators", "empty-members", "members-absent"],
)
def test_red_12_measurement_without_the_if_then_block_the_poisoned_cases_render(
    teams: list[dict], tmp_path: Path
) -> None:
    """THE MEASUREMENT: delete the `if`/`then` block and the cases go green
    at render (the plan names the first two; the third, `members` absent, is
    measured too), so the schema rule, not a render check, is what refuses them.
    (The seeder binary still refuses both, `registry/teams/tests.rs`.)"""
    v = only_teams(teams)
    assert render(v).returncode != 0, "with the rule in place the case must refuse"
    result = render_in(chart_without_the_rule(tmp_path), v)
    assert result.returncode == 0, f"without the rule it must render:\n{result.stderr}"
    assert objects(result.stdout)


# --- the cross-key rules, each with its poison -------------------------------


def poisoned(mutate) -> dict:  # noqa: ANN001
    r = team_registry()
    mutate(r)
    return team_values(r)


@pytest.mark.parametrize(
    ("mutate", "needle"),
    [
        (lambda r: r["roots"][0].__setitem__("grants", [{"team": "administrators", "level": "read"}]),
         "registry.roots[0]"),
        (lambda r: r["roots"][0].__setitem__("grants", []),
         "registry.roots[0]"),
        (lambda r: r["projects"][0].__setitem__("grants", [{"team": "ghosts", "level": "read"}]),
         "registry.projects[0].grants[0].team \"ghosts\""),
        (lambda r: r["projects"][0].__setitem__(
            "grants",
            [{"team": "estate-runners", "level": "read"}, {"team": "estate-runners", "level": "readWrite"}]),
         "registry.projects[0].grants[1].team \"estate-runners\" on \"yadgarhq/gateway\" is named more than once"),
        (lambda r: r["projects"][0].__setitem__("grants", [{"team": "estate-runners", "level": "write"}]),
         "registry.projects[0].grants[0].level"),
        (lambda r: r["teams"].append({"name": "estate-runners", "members": [RUNNER]}),
         "registry.teams[2].name \"estate-runners\" is declared twice"),
        (lambda r: r["teams"].append({"name": "Bad_Name", "members": [RUNNER]}),
         "registry.teams[2].name \"Bad_Name\""),
        (lambda r: r["teams"].append({"name": "x" * 64, "members": [RUNNER]}),
         "registry.teams[2].name"),
        (lambda r: r["teams"][1].__setitem__("members", [RUNNER, RUNNER]),
         "registry.teams[1].members[1]"),
        (lambda r: r.pop("allowMassRevoke"),
         "registry.allowMassRevoke must be set to true or false"),
        (lambda r: r.__setitem__("allowMassRevoke", "false"),
         "registry.allowMassRevoke must be set to true or false"),
    ],
)
def test_a_bad_team_declaration_is_refused_naming_the_key(mutate, needle: str) -> None:  # noqa: ANN001
    refuses(poisoned(mutate), needle)


@pytest.mark.parametrize(
    "member",
    [
        "0192F3C4-AAAA-7BBB-8CCC-0123456789AB",  # uppercase hex
        "0192f3c4-aaaa-4bbb-8ccc-0123456789ab",  # uuidv4
        "0192f3c4-aaaa-7bbb-cccc-0123456789ab",  # variant nibble c
        "0192f3c4-aaaa-7bbb-8ccc-0123456789a",  # 35 characters
        "00000000-0000-0000-0000-000000000000",  # the nil uuid
        "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab",  # the wire prefix
    ],
)
def test_red_13_a_member_that_is_not_a_lowercase_canonical_uuidv7_is_refused(member: str) -> None:
    v = poisoned(lambda r: r["teams"][1].__setitem__("members", [member]))
    refuses(v, "registry.teams[1].members[0]")


def test_a_team_row_that_is_not_a_map_is_refused_not_crashed() -> None:
    refuses(poisoned(lambda r: r["teams"].append("administrators")), "registry.teams[2] must be a map")


def test_teams_that_is_not_a_list_is_refused() -> None:
    v = poisoned(lambda r: r.__setitem__("teams", {"name": "administrators"}))
    refuses(v, "registry.teams")


def test_teams_declared_without_the_iam_authority_is_refused_naming_the_key() -> None:
    v = team_values()
    del v["iam"]["tls"]["caSecret"]
    refuses(v, "iam.tls.caSecret")


def test_teams_declared_with_an_iam_timeout_outside_the_dial_range_is_refused() -> None:
    v = team_values()
    v["iam"]["requestTimeoutSeconds"] = 60
    refuses(v, "requestTimeoutSeconds")


def test_an_unknown_key_under_teams_or_grants_is_refused_by_the_schema() -> None:
    v = poisoned(lambda r: r["teams"][1].__setitem__("memberz", [RUNNER]))
    refuses(v, "memberz")
    v = poisoned(lambda r: r["projects"][0]["grants"][0].__setitem__("lvl", "read"))
    refuses(v, "lvl")


def test_the_iam_block_is_closed() -> None:
    v = team_values()
    v["iam"]["hots"] = "typo"
    refuses(v, "hots")


# --- the deadline outlasts BOTH calls when teams are declared ----------------


def test_a_deadline_that_covers_one_call_but_not_two_refuses_once_teams_are_declared() -> None:
    # One call: 10 x (45 + 6) = 510. Two calls: 1020.
    v = team_values(job={"activeDeadlineSeconds": 600, "backoffLimit": 2})
    refuses(v, "job.activeDeadlineSeconds (600) is shorter than the retry loop")


def test_the_default_deadline_covers_both_calls_at_the_default_knobs() -> None:
    assert "Job" in rendered(team_values())
