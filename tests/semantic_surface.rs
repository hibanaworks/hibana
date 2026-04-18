#![cfg(feature = "std")]

use serde_json::{Map, Value};
use std::{
    env, fs,
    mem::MaybeUninit,
    path::{Path, PathBuf},
    slice,
};

type JsonMap = Map<String, Value>;
type LineBuf = FixedList<String, 1024>;
type ItemBuf<'a> = FixedList<&'a Value, 1024>;

#[derive(Debug)]
struct FixedList<T, const N: usize> {
    len: usize,
    items: [MaybeUninit<T>; N],
}

impl<T, const N: usize> FixedList<T, N> {
    fn new() -> Self {
        Self {
            len: 0,
            items: unsafe { MaybeUninit::<[MaybeUninit<T>; N]>::uninit().assume_init() },
        }
    }

    fn push(&mut self, value: T) {
        assert!(self.len < N, "fixed list capacity exceeded");
        self.items[self.len].write(value);
        self.len += 1;
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.items.as_ptr() as *const T, self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.items.as_mut_ptr() as *mut T, self.len) }
    }

    fn iter(&self) -> slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

impl<T, const N: usize> Drop for FixedList<T, N> {
    fn drop(&mut self) {
        for idx in 0..self.len {
            unsafe { self.items[idx].assume_init_drop() };
        }
    }
}

impl<T, const N: usize> std::ops::Index<usize> for FixedList<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

fn collect_fixed<T, const N: usize>(iter: impl IntoIterator<Item = T>) -> FixedList<T, N> {
    let mut out = FixedList::new();
    for item in iter {
        out.push(item);
    }
    out
}

fn join_display<T>(
    iter: impl IntoIterator<Item = T>,
    sep: &str,
    mut render: impl FnMut(T) -> String,
) -> String {
    let mut out = String::new();
    let mut first = true;
    for item in iter {
        if !first {
            out.push_str(sep);
        }
        first = false;
        out.push_str(&render(item));
    }
    out
}

fn join_strs<'a>(iter: impl IntoIterator<Item = &'a str>, sep: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for item in iter {
        if !first {
            out.push_str(sep);
        }
        first = false;
        out.push_str(item);
    }
    out
}

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

    let root_items: ItemBuf<'_> =
        collect_fixed(module_item_ids(root).iter().map(|id| get_item(index, id)));
    assert_snapshot(
        &manifest_dir.join(".github/allowlists/lib-public-api.txt"),
        &render_root_surface(root_items.as_slice()),
        "crate root",
    );

    let g_module = root_items
        .iter()
        .copied()
        .find(|item| item["name"].as_str() == Some("g"))
        .expect("crate root must expose g");
    let g_items: ItemBuf<'_> = collect_fixed(
        module_item_ids(g_module)
            .iter()
            .map(|id| get_item(index, id)),
    );
    assert_snapshot(
        &manifest_dir.join(".github/allowlists/g-public-api.txt"),
        &render_g_surface(g_items.as_slice()),
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

fn assert_snapshot(path: &Path, actual: &LineBuf, label: &str) {
    let expected = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read allowlist {}: {err}", path.display()));
    let expected_lines: FixedList<String, 1024> = collect_fixed(
        expected
            .lines()
            .map(normalize_ws)
            .filter(|line| !line.is_empty()),
    );
    let actual_lines: FixedList<String, 1024> =
        collect_fixed(actual.iter().map(|line| normalize_ws(line)));

    if expected_lines.as_slice() != actual_lines.as_slice() {
        panic!(
            "semantic public API mismatch for {label}\nexpected:\n{}\nactual:\n{}",
            join_display(expected_lines.iter(), "\n", |line| line.clone()),
            join_display(actual_lines.iter(), "\n", |line| line.clone())
        );
    }
}

fn normalize_ws(input: impl AsRef<str>) -> String {
    let mut normalized = String::new();
    let mut first = true;
    for part in input.as_ref().split_whitespace() {
        if !first {
            normalized.push(' ');
        }
        first = false;
        normalized.push_str(part);
    }

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

fn render_root_surface(items: &[&Value]) -> LineBuf {
    render_items(items, SurfaceMode::Root)
}

fn render_g_surface(items: &[&Value]) -> LineBuf {
    let mut rendered = LineBuf::new();
    let mut ordered: ItemBuf<'_> = collect_fixed(items.iter().copied());
    ordered.as_mut_slice().sort_by_key(|item| span_key(item));
    for item in ordered.iter() {
        let use_item = &item["inner"]["use"];
        let name = use_item["name"]
            .as_str()
            .expect("g surface use items must expose imported names");
        rendered.push(format!("pub use {name};"));
    }
    rendered
}

fn render_file_surface(index: &JsonMap, filename: &str) -> LineBuf {
    let module_name = Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut items: ItemBuf<'_> = collect_fixed(index.values().filter(|item| {
        item["visibility"].as_str() == Some("public")
            && span_filename(item) == Some(filename)
            && !(item["inner"].get("module").is_some()
                && item["name"].as_str() == Some(module_name))
    }));
    items.as_mut_slice().sort_by_key(|item| span_key(item));
    render_items(items.as_slice(), SurfaceMode::File(filename))
}

#[derive(Clone, Copy)]
enum SurfaceMode<'a> {
    Root,
    File(&'a str),
}

fn render_items(items: &[&Value], mode: SurfaceMode<'_>) -> LineBuf {
    let mut rendered = LineBuf::new();
    let mut idx = 0usize;
    while idx < items.len() {
        let item = items[idx];
        if item["inner"].get("use").is_some() {
            let prefix = use_prefix(item).unwrap_or_default();
            let mut group: ItemBuf<'_> = collect_fixed([item]);
            idx += 1;
            while idx < items.len()
                && items[idx]["inner"].get("use").is_some()
                && use_prefix(items[idx]).unwrap_or_default() == prefix
            {
                group.push(items[idx]);
                idx += 1;
            }
            rendered.push(render_use_group(group.as_slice(), mode));
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

    let inputs = join_display(
        sig["inputs"]
            .as_array()
            .expect("function sig must expose inputs")
            .iter(),
        ", ",
        render_input,
    );
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

    let rendered = join_display(params.iter(), ", ", render_generic_param);
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

    let rendered = join_display(predicates.iter(), ", ", render_where_predicate);
    format!(" where {},", rendered)
}

fn render_where_predicate(predicate: &Value) -> String {
    if let Some(bound) = predicate.get("bound_predicate") {
        let bounds = join_display(
            bound["bounds"]
                .as_array()
                .expect("bound predicate must expose bounds")
                .iter(),
            " + ",
            render_bound,
        );
        return format!("{}: {}", render_type(&bound["type"]), bounds);
    }
    if let Some(region) = predicate
        .get("region_predicate")
        .or_else(|| predicate.get("lifetime_predicate"))
    {
        let lifetime = region["lifetime"].as_str().unwrap_or("'_");
        let bounds_value = region
            .get("bounds")
            .or_else(|| region.get("outlives"))
            .expect("lifetime predicate must expose bounds");
        let bounds = join_strs(
            bounds_value
                .as_array()
                .expect("lifetime predicate bounds must be an array")
                .iter()
                .filter_map(Value::as_str),
            " + ",
        );
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

fn render_bound_option(bound: &Value) -> Option<String> {
    if bound.get("use").is_some() {
        return None;
    }
    Some(render_bound(bound))
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
        return format!("({})", join_display(tuple.iter(), ", ", render_type));
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
        let mut parts: FixedList<String, 32> = collect_fixed(
            dyn_trait["traits"]
                .as_array()
                .expect("dyn trait must expose traits")
                .iter()
                .map(render_path),
        );
        if let Some(lifetime) = dyn_trait.get("lifetime").and_then(Value::as_str) {
            parts.push(lifetime.to_owned());
        }
        return format!(
            "dyn {}",
            join_display(parts.iter(), " + ", |part| part.clone())
        );
    }
    if let Some(raw_pointer) = ty.get("raw_pointer") {
        let mutable = if raw_pointer["is_mutable"].as_bool().unwrap_or(false) {
            "mut"
        } else {
            "const"
        };
        return format!("*{} {}", mutable, render_type(&raw_pointer["type"]));
    }
    if let Some(impl_trait) = ty.get("impl_trait").and_then(Value::as_array) {
        return format!(
            "impl {}",
            join_display(
                impl_trait.iter().filter_map(render_bound_option),
                " + ",
                |part| part,
            )
        );
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
        let inputs = join_display(
            decl["inputs"]
                .as_array()
                .expect("function pointer inputs must be present")
                .iter(),
            ", ",
            render_type,
        );
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
        let generic_args = angle["args"]
            .as_array()
            .expect("angle bracketed args must be an array");
        let constraints = angle["constraints"]
            .as_array()
            .expect("angle bracketed constraints must be an array");
        let rendered = join_display(
            generic_args
                .iter()
                .map(render_generic_arg)
                .chain(constraints.iter().map(render_generic_constraint)),
            ", ",
            |part| part,
        );
        return format!("<{}>", rendered);
    }
    if let Some(parenthesized) = args.get("parenthesized") {
        let inputs = join_display(
            parenthesized["inputs"]
                .as_array()
                .expect("parenthesized args must expose inputs")
                .iter(),
            ", ",
            render_type,
        );
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

fn render_generic_constraint(constraint: &Value) -> String {
    let binding = constraint
        .get("binding")
        .expect("generic constraint must expose binding");
    let name = binding["name"]
        .as_str()
        .or_else(|| constraint["name"].as_str())
        .unwrap_or("_");
    let args = binding.get("args").map(render_args).unwrap_or_default();
    let head = format!("{name}{args}");
    if let Some(equality) = binding.get("equality") {
        return format!("{head} = {}", render_type(&equality["type"]));
    }
    if let Some(bounds) = binding.get("bounds") {
        return format!(
            "{head}: {}",
            join_display(
                bounds
                    .as_array()
                    .expect("generic constraint bounds must be an array")
                    .iter(),
                " + ",
                render_bound,
            )
        );
    }
    panic!("unsupported generic constraint: {constraint:?}");
}

fn render_use_group(group: &[&Value], mode: SurfaceMode<'_>) -> String {
    let mut entries: FixedList<((u64, usize, u64), String, String), 128> =
        collect_fixed(group.iter().map(|item| {
            let use_item = &item["inner"]["use"];
            (
                use_sort_key(item),
                use_item["name"].as_str().unwrap_or_default().to_owned(),
                use_item["source"].as_str().unwrap_or_default().to_owned(),
            )
        }));
    entries.as_mut_slice().sort_by_key(|entry| entry.0);

    let names: FixedList<&str, 128> = collect_fixed(entries.iter().map(|entry| entry.1.as_str()));
    let prefix = use_prefix(group[0]).unwrap_or_default();

    if prefix == "crate::epf::verifier" && names.as_slice() == ["Header"] {
        return "pub use Header;".to_owned();
    }
    if prefix == "crate::epf::vm" && names.as_slice() == ["Slot"] {
        return "pub use Slot;".to_owned();
    }
    if prefix == "crate::control::types"
        && names.len() == 2
        && names.iter().any(|name| *name == "One")
        && names.iter().any(|name| *name == "Many")
    {
        return "pub use {One, Many};".to_owned();
    }

    if matches!(mode, SurfaceMode::Root | SurfaceMode::File("src/lib.rs")) {
        if entries.len() > 1 {
            let grouped = join_strs(entries.iter().map(|entry| entry.1.as_str()), ", ");
            return format!("pub use {}::{{{}}};", prefix, grouped);
        }
    }

    if matches!(mode, SurfaceMode::File("src/g.rs")) {
        let grouped = join_strs(entries.iter().map(|entry| entry.1.as_str()), ", ");
        if entries.len() == 1 {
            return format!("pub use {};", grouped);
        }
        return format!("pub use {{{}}};", grouped);
    }

    if entries.len() > 1 {
        let grouped = join_strs(entries.iter().map(|entry| entry.1.as_str()), ", ");
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

    let mut violations: FixedList<String, 256> = FixedList::new();

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
        join_display(violations.iter(), "\n", |line| line.clone())
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
