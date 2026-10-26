{{- /*
The declaration document, as JSON. A partial, so `configmap.yaml` renders it and
`render-checks.yaml` measures the same bytes against the size guard.
*/}}
{{- define "seeder.registryJson" -}}
{{- /*
Read through `seeder.registry`, so the presence check runs first.
*/}}
{{- $_ := include "seeder.registry" . -}}
{{- $r := .Values.registry -}}
{{- $teams := eq (include "seeder.hasTeams" .) "true" -}}
{{- $rows := dict "roots" list "projects" list -}}
{{- range $list := list "roots" "projects" -}}
{{- $out := list -}}
{{- range (get $r $list) -}}
{{- $row := dict "path" .path "displayName" (.displayName | default "") "sourceRepo" .sourceRepo -}}
{{- /*
Every row carries `grants`, `[]` when it declares none, so the binary reads one
shape (`teams` is required, so a render without it is refused in
`render-checks.yaml` before this partial matters). A non-list `grants` or a non-map entry renders as empty here; `render-checks.yaml`
refuses it by name, and this partial must not be the thing that crashes first.
*/ -}}
{{- if $teams -}}
{{- $grants := list -}}
{{- if kindIs "slice" .grants -}}
{{- range .grants -}}
{{- if kindIs "map" . -}}
{{- $grants = append $grants (dict "team" .team "level" .level) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- $_ := set $row "grants" $grants -}}
{{- end -}}
{{- $out = append $out $row -}}
{{- end -}}
{{- $_ := set $rows $list $out -}}
{{- end -}}
{{- $doc := dict "roots" $rows.roots "projects" $rows.projects "allowMassArchive" $r.allowMassArchive -}}
{{- if $teams -}}
{{- $listed := list -}}
{{- range $r.teams -}}
{{- if kindIs "map" . -}}
{{- $listed = append $listed (dict "name" .name "members" (.members | default list)) -}}
{{- end -}}
{{- end -}}
{{- $_ := set $doc "teams" $listed -}}
{{- $_ := set $doc "allowMassRevoke" $r.allowMassRevoke -}}
{{- end -}}
{{- $doc | toJson -}}
{{- end -}}

{{- /*
THE ONE PRESENCE CHECK: a refusal naming the key when `registry` is absent or null.
Every template that reads the declaration reads it through here
(`seeder.registryJson` for `configmap.yaml`, and `render-checks.yaml` before its
first rule), so whichever helm renders first refuses with this sentence, never
a type error from `get` on nothing. The schema refuses an ABSENT block first,
in helm's own words; this catches `registry: null`, which passes the schema,
and any render that skips the schema. ADR-0857 contract step: no default and no
quiet skip (ADR-0569 as amended by ADR-0705).
*/}}
{{- define "seeder.registry" -}}
{{- required "registry is required with no default (ADR-0569, ADR-0857). Declare the organisation's registry.roots, registry.projects and registry.allowMassArchive; through the parent chart the key is seeder.registry (plans/the-d35-seeder.md, \"The declaration\")" .Values.registry | kindOf -}}
{{- end -}}

{{- /*
"true" when `registry.teams` is a non-empty list, "" otherwise. An EMPTY list is
refused like an absent one (plans/team-access.md stage 12). A
`teams` that is not a list renders as absent here and is refused by name in
`render-checks.yaml`.
*/}}
{{- define "seeder.hasTeams" -}}
{{- $r := .Values.registry -}}
{{- if and (kindIs "map" $r) (kindIs "slice" $r.teams) -}}
{{- if $r.teams }}true{{ end -}}
{{- end -}}
{{- end -}}
