#!/usr/bin/env python3
"""Comparison worker for scripts/audit-fal-schemas.sh.

Reads the provider config as JSON — produced by
`asset-tap --dump-provider-config fal.ai`, i.e. the app's own serde_yaml parse
with anchors already resolved — and diffs each model's request body + declared
parameters against fal's cached per-endpoint OpenAPI schema. Reading the real
parser's output means the audit can never disagree with the app about what the
YAML says.

Findings are suppressed per model by scripts/audit-fal-allowlist.json:

    {"<model id>": {
       "unexposed": ["<schema property>", ...],
       "mismatch":  ["<param>.<field>", ...],   # field: enum|default|min|max|type
       "note": "why these are deliberate"
     }}

`mismatch` entries are scoped to one field of one parameter, so allowlisting
`seed.min` still surfaces a later `seed.enum` drift.
"""

from __future__ import annotations

import json
import re
import sys


def load_config(path: str):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


# --- Audit -----------------------------------------------------------------

TEMPLATE = re.compile(r"^\$\{[^}]+\}$")


def models(doc) -> list[dict]:
    out = []
    for stage in ("text_to_image", "image_to_3d"):
        for model in doc.get(stage) or []:
            out.append(model)
    return out


def input_schema(spec: dict):
    """Resolve the endpoint's input schema: prefer the POST request body $ref."""
    for path, ops in (spec.get("paths") or {}).items():
        post = (ops or {}).get("post") or {}
        ref = (
            ((post.get("requestBody") or {}).get("content") or {})
            .get("application/json", {})
            .get("schema", {})
            .get("$ref")
        )
        if ref and ref.startswith("#/components/schemas/"):
            name = ref.rsplit("/", 1)[-1]
            schema = (spec.get("components", {}).get("schemas") or {}).get(name)
            if schema is not None:
                return name, schema, path
    # Fall back to the lone *Input schema.
    schemas = spec.get("components", {}).get("schemas") or {}
    candidates = [n for n in schemas if n.endswith("Input")]
    if len(candidates) == 1:
        return candidates[0], schemas[candidates[0]], None
    raise SystemExit(f"could not resolve input schema (candidates: {candidates})")


def prop_types(prop: dict) -> list[str]:
    """Schema types for a property, flattening anyOf and dropping 'null'."""
    if "type" in prop:
        return [prop["type"]]
    types = []
    for alt in prop.get("anyOf") or []:
        t = alt.get("type")
        if t and t != "null":
            types.append(t)
    return types


def prop_enum(prop: dict):
    if "enum" in prop:
        return prop["enum"]
    for alt in prop.get("anyOf") or []:
        if "enum" in alt:
            return alt["enum"]
    return None


def prop_bound(prop: dict, key: str):
    if key in prop:
        return prop[key]
    for alt in prop.get("anyOf") or []:
        if key in alt:
            return alt[key]
    return None


# Our declared parameter types mapped onto the JSON Schema types they may back.
TYPE_OK = {
    "float": {"number", "integer"},
    "integer": {"integer", "number"},
    "boolean": {"boolean"},
    "string": {"string"},
    "select": {"string", "integer", "number", "boolean"},
}


def audit_model(model: dict, spec: dict, allow: dict):
    name, schema, path = input_schema(spec)
    props = schema.get("properties") or {}
    required = schema.get("required") or []

    body = ((model.get("request") or {}).get("body")) or {}
    params = {p["name"]: p for p in (model.get("parameters") or [])}
    sent = set(body) | set(params)

    findings: dict[str, list] = {
        "sent_but_unknown": [],
        "unexposed": [],
        "mismatch": [],
        "required_missing": [],
    }

    for key in sorted(sent):
        if key not in props:
            findings["sent_but_unknown"].append(
                {"name": key, "where": "body+parameters" if key in body and key in params else ("body" if key in body else "parameters")}
            )

    allowed_unexposed = set(allow.get("unexposed") or [])
    allowed_mismatch = set(allow.get("mismatch") or [])
    for key in sorted(props):
        if key in sent or key in allowed_unexposed:
            continue
        p = props[key]
        findings["unexposed"].append(
            {
                "name": key,
                "type": "|".join(prop_types(p)) or "?",
                "default": p.get("default"),
                "enum": prop_enum(p),
                "description": (p.get("description") or "").strip(),
            }
        )

    for key in sorted(params):
        p = props.get(key)
        if p is None:
            continue
        ours = params[key]
        diffs = []

        schema_enum = prop_enum(p)
        our_options = ours.get("options")
        if our_options is not None or schema_enum is not None:
            if schema_enum is None:
                diffs.append({"field": "enum", "ours": our_options, "schema": None})
            elif our_options is None:
                diffs.append({"field": "enum", "ours": None, "schema": schema_enum})
            else:
                # Order is a UI choice, not a contract — compare as sets. An
                # explicit "" option is our allow_unset escape hatch.
                ours_set = {o for o in our_options if o != ""}
                schema_set = {o for o in schema_enum if o != ""}
                if ours_set != schema_set:
                    diffs.append(
                        {
                            "field": "enum",
                            "ours": our_options,
                            "schema": schema_enum,
                            "only_ours": sorted(map(repr, ours_set - schema_set)),
                            "only_schema": sorted(map(repr, schema_set - ours_set)),
                        }
                    )

        # The dumped config always carries every optional field, as an
        # explicit null when unset — so presence is `is not None`, not `in`.
        if ours.get("default") is not None or "default" in p:
            ours_default = ours.get("default")
            schema_default = p.get("default")
            if not _same_number(ours_default, schema_default):
                diffs.append({"field": "default", "ours": ours_default, "schema": schema_default})

        for our_key, schema_key in (("min", "minimum"), ("max", "maximum")):
            ours_bound = ours.get(our_key)
            schema_bound = prop_bound(p, schema_key)
            if ours_bound is None and schema_bound is None:
                continue
            if not _same_number(ours_bound, schema_bound):
                excl = prop_bound(p, "exclusiveMinimum" if our_key == "min" else "exclusiveMaximum")
                diffs.append(
                    {
                        "field": our_key,
                        "ours": ours_bound,
                        "schema": schema_bound,
                        **({"schema_exclusive": excl} if excl is not None else {}),
                    }
                )

        types = prop_types(p)
        ours_type = ours.get("type")
        if types and ours_type in TYPE_OK and not (TYPE_OK[ours_type] & set(types)):
            diffs.append({"field": "type", "ours": ours_type, "schema": "|".join(types)})

        # Suppress per-field, not per-parameter: `seed.min` in the allowlist
        # hides only the bound, so an enum or type change on the same param
        # still fails the gate.
        diffs = [d for d in diffs if f"{key}.{d['field']}" not in allowed_mismatch]

        if diffs:
            findings["mismatch"].append({"name": key, "diffs": diffs})

    for key in required:
        if key not in body:
            findings["required_missing"].append(key)

    return name, path, findings


def _same_number(a, b):
    if isinstance(a, bool) or isinstance(b, bool):
        return a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return float(a) == float(b)
    return a == b


def has_findings(f: dict) -> bool:
    return any(f[k] for k in f)


def fmt(value) -> str:
    return json.dumps(value, ensure_ascii=False)


def main() -> int:
    argv = sys.argv[1:]
    if argv and argv[0] == "--list-ids":
        doc = load_config(argv[1])
        for m in models(doc):
            print(m["id"])
        return 0

    as_json = "--json" in argv
    argv = [a for a in argv if a != "--json"]
    config_path, allowlist_path, cache_dir = argv[0], argv[1], argv[2]

    doc = load_config(config_path)
    allowlist = json.load(open(allowlist_path, encoding="utf-8"))

    report = []
    any_finding = False
    for model in models(doc):
        mid = model["id"]
        cache = f"{cache_dir}/{mid.replace('/', '_')}.json"
        spec = json.load(open(cache, encoding="utf-8"))
        schema_name, path, findings = audit_model(model, spec, allowlist.get(mid) or {})
        any_finding = any_finding or has_findings(findings)
        report.append(
            {"model": mid, "schema": schema_name, "endpoint_path": path, "findings": findings}
        )

    if as_json:
        print(json.dumps({"models": report}, indent=2, ensure_ascii=False))
        return 1 if any_finding else 0

    for entry in report:
        print(f"=== {entry['model']}")
        print(f"    input schema: {entry['schema']}")
        f = entry["findings"]
        if not has_findings(f):
            print("    OK — no findings")
            print()
            continue
        if f["sent_but_unknown"]:
            print("    SENT_BUT_UNKNOWN:")
            for item in f["sent_but_unknown"]:
                print(f"      - {item['name']} (declared in {item['where']})")
        if f["required_missing"]:
            print("    REQUIRED_MISSING:")
            for key in f["required_missing"]:
                print(f"      - {key}")
        if f["mismatch"]:
            print("    MISMATCH:")
            for item in f["mismatch"]:
                print(f"      - {item['name']}:")
                for d in item["diffs"]:
                    extra = ""
                    if "schema_exclusive" in d:
                        extra = f"  [schema exclusive bound: {fmt(d['schema_exclusive'])}]"
                    print(
                        f"          {d['field']}: ours={fmt(d['ours'])} schema={fmt(d['schema'])}{extra}"
                    )
                    if d.get("only_ours"):
                        print(f"            only in ours:  {', '.join(d['only_ours'])}")
                    if d.get("only_schema"):
                        print(f"            only in schema: {', '.join(d['only_schema'])}")
        if f["unexposed"]:
            print("    UNEXPOSED:")
            for item in f["unexposed"]:
                bits = [f"type={item['type']}"]
                if item["default"] is not None:
                    bits.append(f"default={fmt(item['default'])}")
                if item["enum"]:
                    bits.append(f"enum={fmt(item['enum'])}")
                print(f"      - {item['name']} ({', '.join(bits)})")
                if item["description"]:
                    print(f"          {item['description']}")
        print()

    return 1 if any_finding else 0


if __name__ == "__main__":
    sys.exit(main())
