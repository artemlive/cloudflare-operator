use openapiv3::OpenAPI;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{
    env,
    fs::{self, File},
    path::Path,
};
fn find_problematic_schemas(spec: &Value) {
    if let Some(schemas) = spec
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(|s| s.as_object())
    {
        for (name, schema) in schemas {
            if has_allof_with_strings(schema) {
                println!("⚠️  Suspect schema: {}", name);
                println!("    {}", serde_json::to_string_pretty(schema).unwrap_or_default());
            }
        }
    }
}

fn has_allof_with_strings(schema: &Value) -> bool {
    if let Some(all_of) = schema.get("allOf").and_then(|a| a.as_array()) {
        let string_count = all_of
            .iter()
            .filter(|s| s.get("type").and_then(|t| t.as_str()) == Some("string"))
            .count();
        if string_count > 1 {
            return true;
        }
    }
    // Check nested properties too
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        for (_, prop_schema) in props {
            if has_allof_with_strings(prop_schema) {
                return true;
            }
        }
    }
    false
}

/// Recursively walks the OpenAPI spec and fixes allOf patterns that typify can't handle.
///
/// The main problematic patterns are:
/// 1. allOf with $ref + string type (e.g., $ref to uuid schema + {type: string, format: uuid})
/// 2. allOf with enum + nullable string (e.g., {enum: [...]} + {type: string, nullable: true})
/// 3. allOf with anyOf + string constraints (e.g., {anyOf: [string, number]} + {type: string, maxLength: 1000})
/// 4. allOf with redundant string types (e.g., {type: string} + {type: string})
///
/// typify's merge logic panics on these because it doesn't know how to merge
/// two string schemas or a $ref with a string schema.
fn fix_broken_allofs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // First, check if this object has an allOf that needs fixing
            if let Some(all_of) = map.get("allOf") {
                if let Value::Array(items) = all_of {
                    if should_simplify_allof(items) {
                        // Replace the entire object with the simplified schema
                        let simplified = simplify_allof(items);

                        // Remove allOf and merge in the simplified result
                        map.remove("allOf");
                        if let Value::Object(simplified_map) = simplified {
                            for (k, v) in simplified_map {
                                map.insert(k, v);
                            }
                        }
                    }
                }
            }

            // Recurse into all child values (including the potentially modified allOf)
            for val in map.values_mut() {
                fix_broken_allofs(val);
            }
        }
        Value::Array(arr) => {
            // Recurse into array elements
            for val in arr.iter_mut() {
                fix_broken_allofs(val);
            }
        }
        _ => {
            // Primitives (strings, numbers, bools, null) - nothing to do
        }
    }
}

/// Determines if an allOf should be simplified.
///
/// We simplify when there are 2+ items and at least one contains a string type.
/// This catches:
/// - Direct string types: {type: "string", ...}
/// - anyOf/oneOf containing strings: {anyOf: [{type: "string"}, ...]}
/// - Enums (which are implicitly strings): {enum: [...]}
fn should_simplify_allof(items: &[Value]) -> bool {
    if items.len() < 2 {
        return false;
    }

    // Check if ANY item involves a string type
    items.iter().any(|item| {
        // Direct string type
        item.get("type").map(|t| t == "string").unwrap_or(false)
            // Enum without explicit type (implicitly string in OpenAPI)
            || (item.get("enum").is_some() && item.get("type").is_none())
            // anyOf containing a string type
            || item.get("anyOf").map(|v| contains_string_type(v)).unwrap_or(false)
            // oneOf containing a string type  
            || item.get("oneOf").map(|v| contains_string_type(v)).unwrap_or(false)
    })
}

/// Checks if an anyOf/oneOf array contains a string type
fn contains_string_type(value: &Value) -> bool {
    match value {
        Value::Array(arr) => arr
            .iter()
            .any(|v| v.get("type").map(|t| t == "string").unwrap_or(false)),
        _ => false,
    }
}

/// Simplifies an allOf by merging its items into a single schema.
///
/// Strategy:
/// 1. If there's a $ref, use it as the base (it's the "canonical" definition)
/// 2. Otherwise, merge all string-related properties together
/// 3. Preserve nullable, enum, format, and other constraints
/// 4. Handle anyOf specially by keeping it as-is
fn simplify_allof(items: &[Value]) -> Value {
    let mut result = serde_json::Map::new();
    let mut found_ref = false;
    let mut has_enum = false;

    // First pass: look for $ref (the canonical schema reference)
    for item in items {
        if let Value::Object(obj) = item {
            if obj.contains_key("$ref") {
                found_ref = true;
                for (k, v) in obj {
                    result.insert(k.clone(), v.clone());
                }
                break;
            }
        }
    }

    // Second pass: if no $ref found, merge all string properties
    if !found_ref {
        for item in items {
            if let Value::Object(obj) = item {
                if let Some(any_of) = obj.get("anyOf") {
                    result.insert("anyOf".to_string(), any_of.clone());
                    continue;
                }

                // Track if we have an enum
                if obj.contains_key("enum") {
                    has_enum = true;
                }

                for (k, v) in obj {
                    match k.as_str() {
                        "enum" | "format" | "minLength" | "maxLength" | "pattern" => {
                            result.insert(k.clone(), v.clone());
                        }
                        "type" => {
                            if !result.contains_key("type") {
                                result.insert(k.clone(), v.clone());
                            }
                        }
                        _ => {
                            if !result.contains_key(k) {
                                result.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // If we have an enum, remove string-specific constraints that don't apply
    if has_enum || result.contains_key("enum") {
        result.remove("minLength");
        result.remove("maxLength");
        result.remove("pattern");
        result.remove("format");
    }

    let is_nullable = items
        .iter()
        .any(|item| item.get("nullable") == Some(&Value::Bool(true)));
    if is_nullable {
        result.insert("nullable".to_string(), Value::Bool(true));
    }

    Value::Object(result)
}

fn fix_allof_in_schema(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            // Check if this object has a broken allOf
            if let Some(all_of) = obj.get("allOf").and_then(|a| a.as_array()) {
                if should_simplify_allof(all_of) {
                    // Replace the whole thing with a simplified version
                    let simplified = simplify_allof(all_of);
                    obj.remove("allOf");
                    if let Value::Object(simp_obj) = simplified {
                        for (k, v) in simp_obj {
                            obj.insert(k, v);
                        }
                    }
                }
            }

            // Recurse into all values
            for (_, v) in obj.iter_mut() {
                fix_allof_in_schema(v);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                fix_allof_in_schema(item);
            }
        }
        _ => {}
    }
}
/// Fixes schemas that have enum with string validation constraints.
/// typify doesn't handle enum + maxLength/minLength/pattern/format combinations.
fn fix_enum_with_string_constraints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // If this object has an enum, strip string constraints
            if map.contains_key("enum") {
                map.remove("minLength");
                map.remove("maxLength");
                map.remove("pattern");
                // Keep format only if it's not a string validation format
                if let Some(format) = map.get("format") {
                    if let Some(f) = format.as_str() {
                        // These are validation formats, not semantic ones
                        if matches!(f, "email" | "uri" | "hostname" | "ipv4" | "ipv6") {
                            map.remove("format");
                        }
                    }
                }
            }

            // Recurse
            for val in map.values_mut() {
                fix_enum_with_string_constraints(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_enum_with_string_constraints(val);
            }
        }
        _ => {}
    }
}
// Dump ALL allOf schemas to see what's still there
fn dump_all_allofs(value: &Value, path: String) {
    match value {
        Value::Object(map) => {
            if let Some(all_of) = map.get("allOf") {
                if let Value::Array(items) = all_of {
                    // Only log potentially problematic ones
                    let has_string = items
                        .iter()
                        .any(|item| item.get("type").map(|t| t == "string").unwrap_or(false));
                    if items.len() >= 2 && has_string {
                        println!(
                            "🔍 SUSPICIOUS allOf at {}: {}",
                            path,
                            serde_json::to_string(all_of).unwrap()
                        );
                    }
                }
            }
            for (key, val) in map {
                dump_all_allofs(val, format!("{}.{}", path, key));
            }
        }
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                dump_all_allofs(val, format!("{}[{}]", path, i));
            }
        }
        _ => {}
    }
}

/// Fixes enums that have values which can't become valid Rust identifiers.
/// typify needs to convert enum values to variant names, but things like
/// "<", "<=", ">" etc. can't be valid Rust identifiers.
fn fix_invalid_enum_values(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(enum_val) = map.get_mut("enum") {
                if let Value::Array(variants) = enum_val {
                    for variant in variants.iter_mut() {
                        if let Value::String(s) = variant {
                            // Replace problematic operator values
                            let replacement = match s.as_str() {
                                "<" => "lt",
                                "<=" => "lte",
                                ">" => "gt",
                                ">=" => "gte",
                                "==" => "eq",
                                "!=" => "neq",
                                "=" => "assign",
                                "~" => "tilde",
                                "~*" => "tilde_star",
                                "*" => "star",
                                "" => "empty",
                                _ => continue,
                            };
                            *s = replacement.to_string();
                        }
                    }
                }
            }

            // Recurse
            for val in map.values_mut() {
                fix_invalid_enum_values(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_invalid_enum_values(val);
            }
        }
        _ => {}
    }
}

/// Fixes enums that have case-insensitive duplicate values.
/// typify converts enum values to PascalCase variants, so "count" and "COUNT"
/// both become "Count", causing a collision.
fn fix_duplicate_enum_variants(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(enum_val) = map.get_mut("enum") {
                if let Value::Array(variants) = enum_val {
                    // Track seen values (lowercase) and keep first occurrence
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let mut new_variants = Vec::new();

                    for variant in variants.iter() {
                        if let Value::String(s) = variant {
                            let lower = s.to_lowercase();
                            if !seen.contains(&lower) {
                                seen.insert(lower);
                                new_variants.push(variant.clone());
                            }
                        } else {
                            // Keep non-string variants as-is
                            new_variants.push(variant.clone());
                        }
                    }

                    *variants = new_variants;
                }
            }

            // Recurse
            for val in map.values_mut() {
                fix_duplicate_enum_variants(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_duplicate_enum_variants(val);
            }
        }
        _ => {}
    }
}

/// Fixes schemas with invalid default values that typify can't handle.
///
/// There are several patterns in the Cloudflare OpenAPI spec that cause issues:
///
/// 1. **anyOf with conflicting top-level type**: A schema has both `anyOf: [...]` and
///    a top-level `type: "number"`. This is ambiguous — the anyOf already defines the
///    valid types, so the top-level type conflicts. We remove the top-level type.
///    Example: `dns-records_ttl` has `anyOf: [{type: number}, {enum: [1], type: number}]`
///    plus a top-level `type: "number"`.
///
/// 2. **default value not in enum**: A schema has an enum but the default value isn't
///    one of the enum variants. This happens especially after our `fix_invalid_enum_values`
///    replaces `""` with `"empty"` — the default still says `""` but the enum now has
///    `"empty"`. We update the default to match, or remove it if there's no match.
///    Example: `load-balancing_steering_policy` has `default: ""` but enum contains `"empty"`.
///
/// 3. **default type doesn't match schema type**: A schema declares `type: "array"` but
///    has a string default value, or similar type mismatches. typify validates these
///    and fails. We remove the invalid default.
///    Example: `healthchecks_http_config.expected_codes` has `type: "array"` but `default: "200"`.
fn fix_invalid_defaults(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Fix 1: anyOf with conflicting top-level type
            // When anyOf is present, it defines the valid types — the top-level type is redundant
            // and can cause typify to fail when trying to reconcile them.
            if map.contains_key("anyOf") && map.contains_key("type") {
                println!("DEBUG: Removing type from schema with anyOf");
                map.remove("type");
            }
            // Fix 2: default value not in enum
            // After we rename enum values (like "" -> "empty"), defaults may become invalid.
            // We either update the default to the renamed value, or remove it entirely.
            if let (Some(default_val), Some(enum_val)) = (map.get("default"), map.get("enum")) {
                if let (Value::String(def), Value::Array(variants)) = (default_val, enum_val) {
                    let in_enum = variants.iter().any(|v| v.as_str() == Some(def.as_str()));
                    if !in_enum {
                        // Special case: we renamed "" to "empty" in fix_invalid_enum_values
                        if def.is_empty() && variants.iter().any(|v| v.as_str() == Some("empty")) {
                            map.insert("default".to_string(), json!("empty"));
                        } else {
                            // No valid mapping found — remove the invalid default
                            map.remove("default");
                        }
                    }
                }
            }

            // Fix 3: default is string but type is array
            // A string default for an array type is invalid and will cause typify to fail.
            if let (Some(Value::String(_)), Some(Value::String(type_str))) =
                (map.get("default"), map.get("type"))
            {
                if type_str == "array" {
                    map.remove("default");
                }
            }

            // Recurse into all nested values
            for val in map.values_mut() {
                fix_invalid_defaults(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_invalid_defaults(val);
            }
        }
        _ => {}
    }
}

/// Fixes anyOf patterns that typify can't handle.
/// Specifically, anyOf with numeric types and numeric enums.
fn fix_problematic_anyof(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(any_of) = map.get("anyOf") {
                if let Value::Array(items) = any_of {
                    // Check if this is a "number or specific number" pattern
                    // e.g., anyOf: [{type: number, min: 30, max: 86400}, {enum: [1], type: number}]
                    let all_numbers = items.iter().all(|item| {
                        item.get("type").and_then(|t| t.as_str()) == Some("number")
                            || item.get("type").and_then(|t| t.as_str()) == Some("integer")
                    });

                    if all_numbers && items.len() >= 2 {
                        // Simplify to just a number type
                        map.remove("anyOf");
                        map.insert("type".to_string(), json!("number"));
                    }
                }
            }

            for val in map.values_mut() {
                fix_problematic_anyof(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_problematic_anyof(val);
            }
        }
        _ => {}
    }
}

/// Fixes request bodies that are missing a schema.
/// progenitor requires all request bodies to have a schema defined.
fn fix_missing_request_body_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Check if this is a requestBody with content but no schema
            if let Some(content) = map.get_mut("content") {
                if let Value::Object(content_map) = content {
                    for (_media_type, media_obj) in content_map.iter_mut() {
                        if let Value::Object(media_map) = media_obj {
                            // If there's no schema, add an empty object schema
                            if !media_map.contains_key("schema") {
                                media_map.insert("schema".to_string(), json!({"type": "object"}));
                            }
                        }
                    }
                }
            }

            // Recurse
            for val in map.values_mut() {
                fix_missing_request_body_schema(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_missing_request_body_schema(val);
            }
        }
        _ => {}
    }
}

/// Fixes content types that progenitor doesn't support.
/// Converts multipart/form-data to application/json.
fn fix_unsupported_content_types(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(content) = map.get_mut("content") {
                if let Value::Object(content_map) = content {
                    // Check for multipart/form-data
                    if let Some(multipart) = content_map.remove("multipart/form-data") {
                        // Convert to application/json if not already present
                        if !content_map.contains_key("application/json") {
                            content_map.insert("application/json".to_string(), multipart);
                        }
                    }
                    // Also handle application/octet-stream
                    if let Some(octet) = content_map.remove("application/octet-stream") {
                        if !content_map.contains_key("application/json") {
                            // For binary data, use a simple object schema
                            content_map.insert(
                                "application/json".to_string(),
                                json!({
                                    "schema": {"type": "string", "format": "binary"}
                                }),
                            );
                        }
                    }
                }
            }

            // Recurse
            for val in map.values_mut() {
                fix_unsupported_content_types(val);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                fix_unsupported_content_types(val);
            }
        }
        _ => {}
    }
}

fn generate_id(method: &str, path: &str) -> String {
    let clean_path = path.replace(['{', '}'], "").replace(['/', '-'], "_");

    format!("{}_{}", method.to_lowercase(), clean_path)
        .trim_start_matches('_')
        .replace("__", "_")
}

fn is_http_method(m: &str) -> bool {
    matches!(
        m.to_lowercase().as_str(),
        "get" | "post" | "put" | "delete" | "patch" | "options" | "head"
    )
}
fn collapse_content_map(content: &mut serde_json::Map<String, Value>) {
    if content.len() <= 1 {
        return;
    }

    // pick one content-type to keep
    let keep = if content.contains_key("application/json") {
        "application/json"
    } else if content.contains_key("text/event-stream") {
        "text/event-stream"
    } else {
        // keep first one if no preference
        content.keys().next().unwrap().as_str()
    }
    .to_string();

    let keep_val = content.remove(&keep).unwrap();
    content.clear();
    content.insert(keep, keep_val);
}

fn assert_no_multi_response_content_types(spec: &Value) {
    let mut n = 0;

    // ONLY check components.responses — progenitor resolves refs there
    let Some(responses) = spec
        .get("components")
        .and_then(|c| c.get("responses"))
        .and_then(|r| r.as_object())
    else {
        return;
    };

    for (name, resp) in responses {
        let Some(content) = resp.get("content").and_then(|c| c.as_object()) else {
            continue;
        };

        if content.len() > 1 {
            n += 1;
            println!(
                "STILL_MULTI_COMPONENT_RESPONSE: {} -> {:?}",
                name,
                content.keys().collect::<Vec<_>>()
            );
        }
    }

    assert!(
        n == 0,
        "multi-content responses still exist in components.responses"
    );
}

fn strip_non_success_response_bodies(spec: &mut Value) {
    let Some(paths) = spec.get_mut("paths").and_then(|p| p.as_object_mut()) else {
        return;
    };

    for (_path, methods) in paths {
        let Some(methods) = methods.as_object_mut() else {
            continue;
        };

        for (method, op) in methods {
            if !is_http_method(method) {
                continue;
            }

            let Some(responses) = op.get_mut("responses").and_then(|r| r.as_object_mut()) else {
                continue;
            };

            for (status, resp) in responses {
                // keep only 2xx responses
                let is_success = status.starts_with('2');

                if !is_success {
                    resp.as_object_mut().map(|r| r.remove("content"));
                }
            }
        }
    }
}

fn find_operations_with_multiple_response_bodies(spec: &Value) {
    let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) else {
        return;
    };

    for (path, methods) in paths {
        let Some(methods) = methods.as_object() else {
            continue;
        };

        for (method, op) in methods {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "options" | "head"
            ) {
                continue;
            }

            let Some(responses) = op.get("responses").and_then(|r| r.as_object()) else {
                continue;
            };

            let mut count = 0;
            let mut statuses = Vec::new();

            for (status, resp) in responses {
                if let Some(content) = resp.get("content") {
                    if content.as_object().map(|c| !c.is_empty()).unwrap_or(false) {
                        count += 1;
                        statuses.push(status.clone());
                    }
                }
            }

            if count > 1 {
                println!(
                    "❌ MULTI RESPONSE BODY: {} {} -> {:?}",
                    method.to_uppercase(),
                    path,
                    statuses
                );
            }
        }
    }
}

fn force_json_single_success_response(spec: &mut Value) {
    let Some(paths) = spec.get_mut("paths").and_then(|p| p.as_object_mut()) else {
        return;
    };

    for (_path, methods) in paths {
        let Some(methods) = methods.as_object_mut() else {
            continue;
        };

        for (_method, op) in methods {
            let Some(responses) = op.get_mut("responses").and_then(|r| r.as_object_mut()) else {
                continue;
            };

            // 1) Strip non-2xx bodies (optional but helps avoid multi-response-type explosions)
            for (status, resp) in responses.iter_mut() {
                if !status.starts_with('2') {
                    if let Some(obj) = resp.as_object_mut() {
                        obj.remove("content");
                    }
                }
            }

            // 2) Collect distinct 2xx application/json schemas
            let mut schemas: Vec<String> = Vec::new();

            for (status, resp) in responses.iter() {
                if !status.starts_with('2') {
                    continue;
                }
                let Some(content) = resp.get("content").and_then(|c| c.as_object()) else {
                    continue;
                };
                let Some(app_json) = content.get("application/json") else {
                    continue;
                };
                let Some(schema) = app_json.get("schema") else {
                    continue;
                };

                schemas.push(schema.to_string());
            }

            schemas.sort();
            schemas.dedup();

            // 3) If >1 distinct success schema => squash ONLY this operation to "any JSON"
            if schemas.len() > 1 {
                // pick the first 2xx to keep (stable)
                let keep_status = match responses.keys().find(|s| s.starts_with('2')) {
                    Some(s) => s.clone(),
                    None => continue,
                };

                // compute which other 2xx responses to drop BEFORE we take a mutable borrow
                let to_remove: Vec<String> = responses
                    .keys()
                    .filter(|s| s.starts_with('2') && *s != &keep_status)
                    .cloned()
                    .collect();

                // mutate kept response in a tight scope
                {
                    let keep_resp = responses.get_mut(&keep_status).unwrap();

                    if let Some(content) = keep_resp.get_mut("content").and_then(|c| c.as_object_mut()) {
                        // keep ONLY application/json
                        let json_entry = content.remove("application/json");
                        content.clear();

                        if let Some(mut json_entry) = json_entry {
                            if let Some(m) = json_entry.as_object_mut() {
                                // schema = {}  (means: any JSON)
                                m.insert("schema".to_string(), json!({}));
                            }
                            content.insert("application/json".to_string(), json_entry);
                        }
                    }
                } // <- mutable borrow dropped here

                // now it's safe to mutate map structure
                for k in to_remove {
                    responses.remove(&k);
                }
            }
        }
    }
}
fn main() {
    println!("HERE");

    let src = "/Users/artemlive/ops-stuff/repos/oss/cloudflare-operator/openapi.json";
    println!("cargo:rerun-if-changed={}", src);
    let file = File::open(src).unwrap();
    let mut spec_json: Value = serde_json::from_reader(file).unwrap();

    // Generate operationIds for endpoints missing them
    if let Some(paths) = spec_json.get_mut("paths").and_then(|p| p.as_object_mut()) {
        paths.iter_mut().for_each(|(path_url, methods)| {
            if let Some(methods_map) = methods.as_object_mut() {
                methods_map
                    .iter_mut()
                    .filter(|(method, _)| is_http_method(method))
                    .filter(|(_, op)| op.get("operationId").is_none())
                    .for_each(|(method, op)| {
                        op["operationId"] = json!(generate_id(method, path_url));
                    });
            }
        });
    }

    // Apply all fixes
    fix_broken_allofs(&mut spec_json);
    fix_enum_with_string_constraints(&mut spec_json);
    fix_invalid_enum_values(&mut spec_json);
    fix_duplicate_enum_variants(&mut spec_json);
    fix_invalid_defaults(&mut spec_json);
    fix_problematic_anyof(&mut spec_json);
    fix_missing_request_body_schema(&mut spec_json);
    fix_unsupported_content_types(&mut spec_json);
    force_json_single_success_response(&mut spec_json);
    // Dump for debugging
    std::fs::write(
        "/tmp/patched_spec.json",
        serde_json::to_string_pretty(&spec_json).unwrap(),
    )
    .unwrap();
    println!("Wrote patched spec to /tmp/patched_spec.json");
    find_operations_with_multiple_response_bodies(&spec_json);
    println!(">>> Parsing into OpenAPI struct...");
    let spec: OpenAPI =
        serde_json::from_value(spec_json).expect("Could not parse patched JSON into OpenAPI struct");
    println!(">>> OpenAPI struct parsed OK");

    println!(">>> Creating generator...");
    let mut settings = progenitor::GenerationSettings::default();
    settings.with_interface(progenitor::InterfaceStyle::Builder);
    let mut generator = progenitor::Generator::new(&settings);

    println!(">>> Generating tokens...");
    let tokens = match generator.generate_tokens(&spec) {
        Ok(t) => {
            println!(">>> Tokens generated OK");
            t
        }
        Err(e) => {
            eprintln!("generate_tokens failed: {:?}", e);
            panic!("Failed to generate tokens");
        }
    };

    let tokens_str = tokens.to_string();
    std::fs::write("/tmp/generated_tokens.rs", &tokens_str).unwrap();
    println!(
        "Wrote tokens to /tmp/generated_tokens.rs ({} bytes)",
        tokens_str.len()
    );

    let ast: syn::File = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let mut out_file = Path::new(&out_dir).to_path_buf();
    out_file.push("openapi-codegen.rs");

    fs::write(&out_file, content).unwrap();
    println!(">>> Done! Wrote to {:?}", out_file);
}
