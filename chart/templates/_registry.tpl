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
{{- $grants := list -}}
{{- if kindIs "slice" .grants -}}
{{- range .grants -}}
{{- if kindIs "map" . -}}
{{- $grants = append $grants (dict "team" .team "level" .level) -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- $_ := set $row "grants" $grants -}}
{{- $out = append $out $row -}}
{{- end -}}
{{- $_ := set $rows $list $out -}}
{{- end -}}
{{- $doc := dict "roots" $rows.roots "projects" $rows.projects "allowMassArchive" $r.allowMassArchive -}}
{{- $listed := list -}}
{{- range $r.teams -}}
{{- if kindIs "map" . -}}
{{- $listed = append $listed (dict "name" .name "members" (.members | default list)) -}}
{{- end -}}
{{- end -}}
{{- $_ := set $doc "teams" $listed -}}
{{- $_ := set $doc "allowMassRevoke" $r.allowMassRevoke -}}
{{- $doc | toJson -}}
{{- end -}}

{{- /*
THE ONE PRESENCE CHECK: a refusal naming the key when `registry` is absent or null,
and a refusal when it is not a map.
Every template that reads the declaration reads it through here
(`seeder.registryJson` for `configmap.yaml`, and `render-checks.yaml` before its
first rule), so whichever helm renders first refuses with this sentence, never
a type error from `get` on nothing. The schema refuses an ABSENT block first,
in helm's own words; this catches `registry: null`, which passes the schema,
and any render that skips the schema. ADR-0857 contract step: no default and no
quiet skip (ADR-0569 as amended by ADR-0705). The map guard is here too, so the
partial that builds the document never depends on helm rendering
`render-checks.yaml` before it: `get` on a string or a list is a Go type error,
and a render that skipped the schema would end there.
*/}}
{{- define "seeder.registry" -}}
{{- $r := required "registry is required with no default (ADR-0569, ADR-0857). Declare the organisation's registry.roots, registry.projects and registry.allowMassArchive; through the parent chart the key is seeder.registry (plans/the-d35-seeder.md, \"The declaration\")" .Values.registry -}}
{{- if not (kindIs "map" $r) -}}
{{- fail "registry must be a map with `roots`, `projects` and `allowMassArchive` (plans/the-d35-seeder.md, \"The declaration\")" -}}
{{- end -}}
{{- kindOf $r -}}
{{- end -}}
