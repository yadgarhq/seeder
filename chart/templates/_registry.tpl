{{- /*
The declaration document, as JSON. A partial, so `configmap.yaml` renders it and
`render-checks.yaml` measures the same bytes against the size guard.
*/}}
{{- define "seeder.registryJson" -}}
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
"true" when the values declare a `registry` block at all, "" otherwise.

`hasKey`, NOT a truthiness test: `registry: {}` is a declaration with nothing in
it, and it must reach the render checks and be refused (no root, no
`allowMassArchive`) rather than read as "absent" and render nothing. Only a
block that is not there — the ADR-0857 expand step, before the parent sets it —
renders nothing.
*/}}
{{- define "seeder.declared" -}}
{{- if and (hasKey .Values "registry") (not (kindIs "invalid" .Values.registry)) -}}true{{- end -}}
{{- end -}}
