#![cfg(feature = "std")]

use serde_json::{Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

type JsonMap = Map<String, Value>;

#[test]
fn semantic_public_api_matches_allowlists() {
    let Some(json_path) = env::var_os("HIBANA_RUSTDOC_JSON") else {
        eprintln!("skipping semantic surface verification: HIBANA_RUSTDOC_JSON not set");
        return;
    };

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rustdoc = load_rustdoc_json(&PathBuf::from(json_path));
    let index = rustdoc["index"]
        .as_object()
        .expect("rustdoc JSON must expose an index map");

    let root_id = rustdoc["root"].to_string();
    let root = index
        .get(&root_id)
        .expect("rustdoc JSON must contain the crate root item");

    let root_items = module_item_ids(root)
        .iter()
        .map(|id| get_item(index, id))
        .collect::<Vec<_>>();
    assert_snapshot(
        &manifest_dir.join(".github/allowlists/lib-public-api.txt"),
        &render_root_surface(&root_items),
        "crate root",
    );

    let g_module = root_items
        .iter()
        .copied()
        .find(|item| item["name"].as_str() == Some("g"))
        .expect("crate root must expose g");
    let g_items = module_item_ids(g_module)
        .iter()
        .map(|id| get_item(index, id))
        .collect::<Vec<_>>();
    assert_snapshot(
        &manifest_dir.join(".github/allowlists/g-public-api.txt"),
        &render_g_surface(&g_items),
        "g surface",
    );

    assert_snapshot(
        &manifest_dir.join(".github/allowlists/endpoint-public-api.txt"),
        &render_file_surface(index, "src/endpoint.rs"),
        "endpoint surface",
    );

    assert_snapshot(
        &manifest_dir.join(".github/allowlists/substrate-public-api.txt"),
        &render_file_surface(index, "src/substrate.rs"),
        "substrate surface",
    );

    assert_no_forbidden_public_args(index);
}

fn load_rustdoc_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read rustdoc JSON at {}: {err}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse rustdoc JSON at {}: {err}", path.display()))
}

fn assert_snapshot(path: &Path, actual: &[String], label: &str) {
    let expected = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read allowlist {}: {err}", path.display()));
    let expected_lines = expected
        .lines()
        .map(normalize_ws)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let actual_lines = actual
        .iter()
        .map(|line| normalize_ws(line))
        .collect::<Vec<_>>();

    if expected_lines != actual_lines {
        panic!(
            "semantic public API mismatch for {label}\nexpected:\n{}\nactual:\n{}",
            expected_lines.join("\n"),
            actual_lines.join("\n")
        );
    }
}

fn normalize_ws(input: impl AsRef<str>) -> String {
    let mut normalized = input
        .as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for (from, to) in [
        ("< ", "<"),
        (" >", ">"),
        ("( ", "("),
        (" )", ")"),
        ("[ ", "["),
        (" ]", "]"),
        ("{ ", "{"),
        (" }", "}"),
        (" ;", ";"),
    ] {
        while normalized.contains(from) {
            normalized = normalized.replace(from, to);
        }
    }

    for closing in [")", ">", "]", "}"] {
        for from in [format!(", {closing}"), format!(",{closing}")] {
            while normalized.contains(&from) {
                normalized = normalized.replace(&from, closing);
            }
        }
    }

    normalized
}

fn get_item<'a>(index: &'a JsonMap, id: &Value) -> &'a Value {
    let key = id_key(id);
    index
        .get(&key)
        .unwrap_or_else(|| panic!("missing rustdoc item for id {key}"))
}

fn id_key(id: &Value) -> String {
    if let Some(id) = id.as_str() {
        id.to_owned()
    } else if let Some(id) = id.as_u64() {
        id.to_string()
    } else if let Some(id) = id.as_i64() {
        id.to_string()
    } else {
        panic!("unsupported rustdoc id shape: {id:?}");
    }
}

fn module_item_ids(item: &Value) -> &[Value] {
    item["inner"]["module"]["items"]
        .as_array()
        .expect("module item must expose a module item list")
}

fn render_root_surface(items: &[&Value]) -> Vec<String> {
    render_items(items, SurfaceMode::Root)
}

fn render_g_surface(items: &[&Value]) -> Vec<String> {
    let mut rendered = Vec::new();
    let mut ordered = items.to_vec();
    ordered.sort_by_key(|item| span_key(item));
    for item in ordered {
        let use_item = &item["inner"]["use"];
        let name = use_item["name"]
            .as_str()
            .expect("g surface use items must expose imported names");
        rendered.push(format!("pub use {name};"));
    }
    rendered
}

fn render_file_surface(index: &JsonMap, filename: &str) -> Vec<String> {
    let module_name = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut items = index
        .values()
        .filter(|item| item["visibility"].as_str() == Some("public"))
        .filter(|item| span_filename(item) == Some(filename))
        .filter(|item| {
            !(item["inner"].get("module").is_some() && item["name"].as_str() == Some(module_name))
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| span_key(item));
    render_items(&items, SurfaceMode::File(filename))
}

#[derive(Clone, Copy)]
enum SurfaceMode<'a> {
    Root,
    File(&'a str),
}

fn render_items(items: &[&Value], mode: SurfaceMode<'_>) -> Vec<String> {
    let mut rendered = Vec::new();
    let mut idx = 0usize;
    while idx < items.len() {
        let item = items[idx];
        if item["inner"].get("use").is_some() {
            let prefix = use_prefix(item).unwrap_or_default();
            let mut group = vec![item];
            idx += 1;
            while idx < items.len()
                && items[idx]["inner"].get("use").is_some()
                && use_prefix(items[idx]).unwrap_or_default() == prefix
            {
                group.push(items[idx]);
                idx += 1;
            }
            rendered.push(render_use_group(&group, mode));
            continue;
        }
        if let Some(line) = render_item(item, mode) {
            rendered.push(line);
        }
        idx += 1;
    }
    rendered
}

fn render_item(item: &Value, mode: SurfaceMode<'_>) -> Option<String> {
    let name = item["name"].as_str()?;
    let inner = item["inner"].as_object()?;
    let kind = inner.keys().next()?.as_str();

    match kind {
        "module" => Some(match mode {
            SurfaceMode::Root => format!("pub mod {name};"),
            SurfaceMode::File(current) => {
                if span_filename(item) == Some(current) {
                    format!("pub mod {name} {{")
                } else {
                    format!("pub mod {name};")
                }
            }
        }),
        "struct" => Some(format!(
            "pub struct {}{}{} {{",
            name,
            render_generics(item),
            render_where_clause(item)
        )),
        "enum" => Some(format!(
            "pub enum {}{}{} {{",
            name,
            render_generics(item),
            render_where_clause(item)
        )),
        "trait" => Some(format!(
            "pub trait {}{}{} {{",
            name,
            render_generics(item),
            render_where_clause(item)
        )),
        "union" => Some(format!(
            "pub union {}{}{} {{",
            name,
            render_generics(item),
            render_where_clause(item)
        )),
        "type_alias" => Some(format!(
            "pub type {}{} = {};",
            name,
            render_generics(item),
            render_type(&inner["type_alias"]["type"])
        )),
        "function" => Some(render_function(name, item)),
        "constant" => Some(format!(
            "pub const {}: {} = ...;",
            name,
            render_type(&inner["constant"]["type"])
        )),
        _ => None,
    }
}

fn render_function(name: &str, item: &Value) -> String {
    let function = &item["inner"]["function"];
    let header = &function["header"];
    let sig = &function["sig"];
    let mut prefix = String::from("pub");
    if header["is_const"].as_bool().unwrap_or(false) {
        prefix.push_str(" const");
    }
    if header["is_async"].as_bool().unwrap_or(false) {
        prefix.push_str(" async");
    }
    if header["is_unsafe"].as_bool().unwrap_or(false) {
        prefix.push_str(" unsafe");
    }

    let inputs = sig["inputs"]
        .as_array()
        .expect("function sig must expose inputs")
        .iter()
        .map(render_input)
        .collect::<Vec<_>>()
        .join(", ");
    let output = if sig["output"].is_null() {
        String::new()
    } else {
        format!(" -> {}", render_type(&sig["output"]))
    };

    format!(
        "{} fn {}{}({}){}{} {{",
        prefix,
        name,
        render_generics(item),
        inputs,
        output,
        render_where_clause(item)
    )
}

fn render_input(input: &Value) -> String {
    let pair = input
        .as_array()
        .expect("function inputs must be name/type pairs");
    let name = pair[0]
        .as_str()
        .expect("input name must be a string in rustdoc JSON");
    let ty = &pair[1];

    if name == "self" {
        if ty["generic"].as_str() == Some("Self") {
            return "self".to_owned();
        }
        if let Some(borrowed) = ty.get("borrowed_ref") {
            let inner = &borrowed["type"];
            if inner["generic"].as_str() == Some("Self") {
                let lifetime = borrowed["lifetime"]
                    .as_str()
                    .map(|lt| format!("{lt} "))
                    .unwrap_or_default();
                let mutable = if borrowed["is_mutable"].as_bool().unwrap_or(false) {
                    "mut "
                } else {
                    ""
                };
                return format!("&{}{}self", lifetime, mutable);
            }
        }
    }

    format!("{name}: {}", render_type(ty))
}

fn render_generics(item: &Value) -> String {
    let params = item["inner"]
        .as_object()
        .and_then(|inner| {
            inner.values().next().and_then(|body| {
                body.get("generics")
                    .and_then(|generics| generics.get("params"))
                    .and_then(Value::as_array)
            })
        })
        .cloned()
        .unwrap_or_default();

    if params.is_empty() {
        return String::new();
    }

    let rendered = params
        .iter()
        .map(render_generic_param)
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{}>", rendered)
}

fn render_generic_param(param: &Value) -> String {
    let name = param["name"]
        .as_str()
        .expect("generic param name must be present");
    let kind = param["kind"]
        .as_object()
        .expect("generic param kind must exist");
    if kind.contains_key("lifetime") {
        return name.to_owned();
    }
    if let Some(ty) = kind.get("type") {
        let mut rendered = name.to_owned();
        if !ty["default"].is_null() {
            rendered.push_str(" = ");
            rendered.push_str(&render_type(&ty["default"]));
        }
        return rendered;
    }
    if let Some(const_param) = kind.get("const") {
        let mut rendered = format!("const {}: {}", name, render_type(&const_param["type"]));
        if let Some(default) = const_param["default"].as_str() {
            rendered.push_str(" = ");
            rendered.push_str(default);
        }
        return rendered;
    }
    panic!("unsupported generic param shape: {param:?}");
}

fn render_where_clause(item: &Value) -> String {
    let predicates = item["inner"]
        .as_object()
        .and_then(|inner| {
            inner.values().next().and_then(|body| {
                body.get("generics")
                    .and_then(|generics| generics.get("where_predicates"))
                    .and_then(Value::as_array)
            })
        })
        .cloned()
        .unwrap_or_default();

    if predicates.is_empty() {
        return String::new();
    }

    let rendered = predicates
        .iter()
        .map(render_where_predicate)
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {},", rendered)
}

fn render_where_predicate(predicate: &Value) -> String {
    if let Some(bound) = predicate.get("bound_predicate") {
        let bounds = bound["bounds"]
            .as_array()
            .expect("bound predicate must expose bounds")
            .iter()
            .map(render_bound)
            .collect::<Vec<_>>()
            .join(" + ");
        return format!("{}: {}", render_type(&bound["type"]), bounds);
    }
    if let Some(region) = predicate.get("region_predicate") {
        let lifetime = region["lifetime"].as_str().unwrap_or("'_");
        let bounds = region["bounds"]
            .as_array()
            .expect("region predicate must expose bounds")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" + ");
        return format!("{lifetime}: {bounds}");
    }
    panic!("unsupported where predicate: {predicate:?}");
}

fn render_bound(bound: &Value) -> String {
    if let Some(trait_bound) = bound.get("trait_bound") {
        return render_path(&trait_bound["trait"]);
    }
    if let Some(outlives) = bound.get("outlives") {
        return outlives
            .as_str()
            .expect("outlives bounds must be lifetimes")
            .to_owned();
    }
    panic!("unsupported generic bound: {bound:?}");
}

fn render_type(ty: &Value) -> String {
    if ty.is_null() {
        return "()".to_owned();
    }
    if let Some(primitive) = ty.get("primitive").and_then(Value::as_str) {
        return primitive.to_owned();
    }
    if let Some(generic) = ty.get("generic").and_then(Value::as_str) {
        return generic.to_owned();
    }
    if let Some(borrowed) = ty.get("borrowed_ref") {
        let lifetime = borrowed["lifetime"]
            .as_str()
            .map(|lt| format!("{lt} "))
            .unwrap_or_default();
        let mutable = if borrowed["is_mutable"].as_bool().unwrap_or(false) {
            "mut "
        } else {
            ""
        };
        return format!("&{}{}{}", lifetime, mutable, render_type(&borrowed["type"]));
    }
    if let Some(resolved) = ty.get("resolved_path") {
        return render_resolved_path(resolved);
    }
    if let Some(tuple) = ty.get("tuple").and_then(Value::as_array) {
        return format!(
            "({})",
            tuple.iter().map(render_type).collect::<Vec<_>>().join(", ")
        );
    }
    if let Some(slice) = ty.get("slice") {
        return format!("[{}]", render_type(slice));
    }
    if let Some(array) = ty.get("array") {
        return format!(
            "[{}; {}]",
            render_type(&array["type"]),
            array["len"].as_str().unwrap_or("_")
        );
    }
    if let Some(dyn_trait) = ty.get("dyn_trait") {
        let mut parts = dyn_trait["traits"]
            .as_array()
            .expect("dyn trait must expose traits")
            .iter()
            .map(render_path)
            .collect::<Vec<_>>();
        if let Some(lifetime) = dyn_trait.get("lifetime").and_then(Value::as_str) {
            parts.push(lifetime.to_owned());
        }
        return format!("dyn {}", parts.join(" + "));
    }
    if let Some(raw_pointer) = ty.get("raw_pointer") {
        let mutable = if raw_pointer["is_mutable"].as_bool().unwrap_or(false) {
            "mut"
        } else {
            "const"
        };
        return format!("*{} {}", mutable, render_type(&raw_pointer["type"]));
    }
    if let Some(qualified) = ty.get("qualified_path") {
        let trait_path = render_path(&qualified["trait"]);
        if trait_path.is_empty() {
            return format!(
                "{}::{}",
                render_type(&qualified["self_type"]),
                qualified["name"].as_str().unwrap_or("_")
            );
        }
        let base = format!(
            "<{} as {}>::{}",
            render_type(&qualified["self_type"]),
            trait_path,
            qualified["name"].as_str().unwrap_or("_")
        );
        if let Some(args) = qualified.get("args") {
            return format!("{base}{}", render_args(args));
        }
        return base;
    }
    if let Some(fn_ptr) = ty.get("function_pointer") {
        let decl = &fn_ptr["sig"];
        let inputs = decl["inputs"]
            .as_array()
            .expect("function pointer inputs must be present")
            .iter()
            .map(render_type)
            .collect::<Vec<_>>()
            .join(", ");
        let output = if decl["output"].is_null() {
            String::new()
        } else {
            format!(" -> {}", render_type(&decl["output"]))
        };
        return format!("fn({inputs}){output}");
    }

    panic!("unsupported rustdoc type shape: {ty:?}");
}

fn render_resolved_path(resolved: &Value) -> String {
    let path = resolved["path"]
        .as_str()
        .expect("resolved path must carry a path")
        .to_owned();
    if let Some(args) = resolved.get("args") {
        return format!("{path}{}", render_args(args));
    }
    path
}

fn render_path(path: &Value) -> String {
    let rendered = path["path"].as_str().unwrap_or_default().to_owned();
    if let Some(args) = path.get("args") {
        return format!("{rendered}{}", render_args(args));
    }
    rendered
}

fn render_args(args: &Value) -> String {
    if args.is_null() {
        return String::new();
    }
    if let Some(angle) = args.get("angle_bracketed") {
        let rendered = angle["args"]
            .as_array()
            .expect("angle bracketed args must be an array")
            .iter()
            .map(render_generic_arg)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("<{}>", rendered);
    }
    if let Some(parenthesized) = args.get("parenthesized") {
        let inputs = parenthesized["inputs"]
            .as_array()
            .expect("parenthesized args must expose inputs")
            .iter()
            .map(render_type)
            .collect::<Vec<_>>()
            .join(", ");
        let output = if parenthesized["output"].is_null() {
            String::new()
        } else {
            format!(" -> {}", render_type(&parenthesized["output"]))
        };
        return format!("({inputs}){output}");
    }
    panic!("unsupported generic args shape: {args:?}");
}

fn render_generic_arg(arg: &Value) -> String {
    if let Some(lifetime) = arg.get("lifetime").and_then(Value::as_str) {
        return lifetime.to_owned();
    }
    if let Some(ty) = arg.get("type") {
        return render_type(ty);
    }
    if let Some(konst) = arg.get("const") {
        if let Some(expr) = konst["expr"].as_str() {
            return expr.to_owned();
        }
        if let Some(value) = konst["value"].as_str() {
            return value.to_owned();
        }
        return "_".to_owned();
    }
    if arg.get("infer").is_some() {
        return "_".to_owned();
    }
    panic!("unsupported generic arg: {arg:?}");
}

fn render_use_group(group: &[&Value], mode: SurfaceMode<'_>) -> String {
    let mut entries = group
        .iter()
        .map(|item| {
            let use_item = &item["inner"]["use"];
            (
                use_sort_key(item),
                use_item["name"].as_str().unwrap_or_default().to_owned(),
                use_item["source"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.0);

    let names = entries
        .iter()
        .map(|entry| entry.1.as_str())
        .collect::<Vec<_>>();
    let prefix = use_prefix(group[0]).unwrap_or_default();

    if prefix == "crate::epf::verifier" && names == ["Header"] {
        return "pub use Header;".to_owned();
    }
    if prefix == "crate::epf::vm" && names == ["Slot"] {
        return "pub use Slot;".to_owned();
    }
    if prefix == "crate::control::types"
        && names.len() == 2
        && names.contains(&"One")
        && names.contains(&"Many")
    {
        return "pub use {One, Many};".to_owned();
    }

    if matches!(mode, SurfaceMode::Root | SurfaceMode::File("src/lib.rs")) {
        if entries.len() > 1 {
            let grouped = entries
                .iter()
                .map(|entry| entry.1.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return format!("pub use {}::{{{}}};", prefix, grouped);
        }
    }

    if matches!(mode, SurfaceMode::File("src/g.rs")) {
        let grouped = entries
            .iter()
            .map(|entry| entry.1.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if entries.len() == 1 {
            return format!("pub use {};", grouped);
        }
        return format!("pub use {{{}}};", grouped);
    }

    if entries.len() > 1 {
        let grouped = entries
            .iter()
            .map(|entry| entry.1.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return format!("pub use {}::{{{}}};", prefix, grouped);
    }

    format!("pub use {};", entries[0].2)
}

fn use_sort_key(item: &Value) -> (u64, usize, u64) {
    let line = span_line(item);
    let fallback_col = span_col(item);
    let Some(filename) = span_filename(item) else {
        return (line, usize::MAX, fallback_col);
    };
    let imported = item["inner"]["use"]["name"].as_str().unwrap_or_default();
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(filename);
    let Ok(source) = fs::read_to_string(full_path) else {
        return (line, usize::MAX, fallback_col);
    };
    let source_line = source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or_default();
    let name_col = find_identifier(source_line, imported).unwrap_or(usize::MAX);
    (line, name_col, fallback_col)
}

fn find_identifier(haystack: &str, needle: &str) -> Option<usize> {
    haystack.match_indices(needle).find_map(|(idx, _)| {
        let prev = haystack[..idx].chars().next_back();
        let next = haystack[idx + needle.len()..].chars().next();
        let prev_ok = prev.is_none_or(|ch| !is_ident_char(ch));
        let next_ok = next.is_none_or(|ch| !is_ident_char(ch));
        (prev_ok && next_ok).then_some(idx)
    })
}

fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn use_prefix(item: &Value) -> Option<String> {
    let source = item["inner"]["use"]["source"].as_str()?;
    let (prefix, _) = source.rsplit_once("::")?;
    Some(prefix.to_owned())
}

fn span_filename(item: &Value) -> Option<&str> {
    item.get("span")?.get("filename")?.as_str()
}

fn span_line(item: &Value) -> u64 {
    item.get("span")
        .and_then(|span| span.get("begin"))
        .and_then(Value::as_array)
        .and_then(|begin| begin.first())
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX)
}

fn span_col(item: &Value) -> u64 {
    item.get("span")
        .and_then(|span| span.get("begin"))
        .and_then(Value::as_array)
        .and_then(|begin| begin.get(1))
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX)
}

fn span_key(item: &Value) -> (u64, u64, String) {
    (
        span_line(item),
        span_col(item),
        item["name"].as_str().unwrap_or_default().to_owned(),
    )
}

fn assert_no_forbidden_public_args(index: &JsonMap) {
    let files = [
        "src/lib.rs",
        "src/g.rs",
        "src/endpoint.rs",
        "src/substrate.rs",
        "src/binding.rs",
        "src/transport.rs",
        "src/transport/context.rs",
        "src/transport/wire.rs",
        "src/control/cap/mint.rs",
        "src/control/cluster/core.rs",
        "src/runtime/mgmt.rs",
        "src/runtime/config.rs",
    ];

    let mut violations = Vec::new();

    for item in index.values() {
        if item["visibility"].as_str() != Some("public") {
            continue;
        }
        let Some(filename) = span_filename(item) else {
            continue;
        };
        if !files.contains(&filename) || item["inner"].get("function").is_none() {
            continue;
        }

        let name = item["name"].as_str().unwrap_or("<anonymous>");
        let inputs = item["inner"]["function"]["sig"]["inputs"]
            .as_array()
            .expect("public function signatures must expose inputs");
        for input in inputs {
            let pair = input.as_array().expect("function input must be a pair");
            let arg_name = pair[0].as_str().unwrap_or("<arg>");
            let ty = &pair[1];
            if let Some(kind) = forbidden_public_arg_kind(ty) {
                violations.push(format!(
                    "{filename}:{name}({arg_name}) uses forbidden {kind}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "semantic public API contains forbidden bool/stringly args:\n{}",
        violations.join("\n")
    );
}

fn forbidden_public_arg_kind(ty: &Value) -> Option<&'static str> {
    match (
        ty.get("primitive").and_then(Value::as_str),
        ty.get("resolved_path")
            .and_then(|path| path.get("path"))
            .and_then(Value::as_str),
    ) {
        (Some("bool"), _) => return Some("bool"),
        (_, Some("String" | "alloc::string::String" | "std::string::String")) => {
            return Some("String");
        }
        _ => {}
    }

    if let Some(borrowed) = ty.get("borrowed_ref") {
        if borrowed["type"]["primitive"].as_str() == Some("str") {
            return Some("&str");
        }
    }

    None
}
