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
{{- $out = append $out (dict "path" .path "displayName" (.displayName | default "") "sourceRepo" .sourceRepo) -}}
{{- end -}}
{{- $_ := set $rows $list $out -}}
{{- end -}}
{{- dict "roots" $rows.roots "projects" $rows.projects "allowMassArchive" $r.allowMassArchive | toJson -}}
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
