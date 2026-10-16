//! Private, std-only audit of should-panic proof harnesses, not a Rust parser.
//! Unknown harness signatures and malformed lexical input are rejected.
use std::{env, fs, path::Path, process};

mod kani_gate;
mod kani_json;

type Audit<T> = Result<T, String>;

fn tokens(source: &str) -> Audit<Vec<String>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return Err("unterminated block comment".into());
            }
            continue;
        }
        // Raw strings (including byte/C prefixes) must not affect brace depth.
        let raw_start = if bytes[i] == b'r' {
            Some(i + 1)
        } else if (bytes[i] == b'b' || bytes[i] == b'c') && bytes.get(i + 1) == Some(&b'r') {
            Some(i + 2)
        } else {
            None
        };
        if let Some(start) = raw_start {
            let mut quote = start;
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            if bytes.get(quote) == Some(&b'"') {
                let hashes = quote - start;
                i = quote + 1;
                loop {
                    if i >= bytes.len() {
                        return Err("unterminated raw string".into());
                    }
                    if bytes[i] == b'"'
                        && bytes
                            .get(i + 1..i + 1 + hashes)
                            .is_some_and(|s| s.iter().all(|c| *c == b'#'))
                    {
                        i += 1 + hashes;
                        break;
                    }
                    i += 1;
                }
                out.push("<literal>".into());
                continue;
            }
        }
        if bytes[i] == b'"' {
            i += 1;
            loop {
                match bytes.get(i) {
                    None => return Err("unterminated string".into()),
                    Some(b'\\') => i += 2,
                    Some(b'"') => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            out.push("<literal>".into());
            continue;
        }
        // A lifetime has no closing quote. A char literal contains one scalar
        // or an escape, followed by its closing quote.
        if bytes[i] == b'\'' {
            let start = i + 1;
            let mut end = start;
            if bytes.get(end) == Some(&b'\\') {
                end += 1;
                if bytes.get(end) == Some(&b'u') && bytes.get(end + 1) == Some(&b'{') {
                    end += 2;
                    while end < bytes.len() && bytes[end] != b'}' {
                        end += 1;
                    }
                    end += 1;
                } else if bytes.get(end) == Some(&b'x') {
                    end += 3;
                } else {
                    end += 1;
                }
            } else if let Some(c) = source[start..].chars().next() {
                end += c.len_utf8();
            }
            if bytes.get(end) == Some(&b'\'') {
                i = end + 1;
                out.push("<literal>".into());
                continue;
            }
        }
        let c = source[i..].chars().next().unwrap();
        if c == '_' || c.is_alphabetic() {
            let mut start = i;
            if bytes[i..].starts_with(b"r#") {
                i += 2;
                start = i;
            }
            while i < bytes.len() {
                let c = source[i..].chars().next().unwrap();
                if c != '_' && !c.is_alphanumeric() {
                    break;
                }
                i += c.len_utf8();
            }
            if i == start {
                return Err("invalid raw identifier".into());
            }
            out.push(source[start..i].into());
        } else {
            out.push(c.to_string());
            i += c.len_utf8();
        }
    }
    Ok(out)
}

fn at(tokens: &[String], index: usize, expected: &[&str]) -> bool {
    tokens
        .get(index..index + expected.len())
        .is_some_and(|found| {
            found
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
}

fn balanced(tokens: &[String], start: usize) -> Audit<usize> {
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(token.as_str()) {
                    return Err("unbalanced harness delimiters".into());
                }
                if stack.is_empty() {
                    return Ok(index + 1);
                }
            }
            _ => (),
        }
    }
    Err("unterminated should-panic harness or attribute".into())
}

fn audit(source: &str) -> Audit<usize> {
    let t = tokens(source)?;
    if source.contains("kani::assume")
        || (0..t.len()).any(|i| at(&t, i, &["kani", ":", ":", "assume"]))
    {
        return Err(
            "Kani harnesses must construct complete symbolic domains without assumptions".into(),
        );
    }
    let mut count = 0;
    let mut i = 0;
    while i < t.len() {
        if !at(&t, i, &["kani", ":", ":", "should_panic"]) {
            i += 1;
            continue;
        }
        if i < 2 || !at(&t, i - 2, &["#", "["]) || !at(&t, i + 4, &["]"]) {
            return Err("unsupported kani::should_panic attribute shape".into());
        }
        i += 5;
        while at(&t, i, &["#", "["]) {
            i = balanced(&t, i + 1)?;
        }
        if at(&t, i, &["pub"]) {
            i += 1;
            if at(&t, i, &["("]) {
                i = balanced(&t, i)?;
            }
        }
        if at(&t, i, &["unsafe"]) {
            i += 1;
        }
        if !at(&t, i, &["fn"]) || t.get(i + 1).is_none() || !at(&t, i + 2, &["(", ")", "{"]) {
            return Err(
                "unsupported should-panic harness signature; expected fn name() { ... }".into(),
            );
        }
        let name = &t[i + 1];
        let body_start = i + 4;
        let end = balanced(&t, body_start)?;
        for cursor in body_start + 1..end - 1 {
            let token = t[cursor].as_str();
            if matches!(
                token,
                "assert"
                    | "assert_eq"
                    | "assert_ne"
                    | "debug_assert"
                    | "debug_assert_eq"
                    | "debug_assert_ne"
                    | "panic"
                    | "unreachable"
                    | "todo"
                    | "unimplemented"
            ) && at(&t, cursor + 1, &["!"])
            {
                return Err(format!(
                    "should-panic harness {name} may panic before its production call"
                ));
            }
            if token == "invariant" && at(&t, cursor + 1, &["("]) {
                return Err(format!(
                    "should-panic harness {name} may panic before its production call"
                ));
            }
            if at(&t, cursor, &["kani", ":", ":", "any"]) {
                return Err(format!(
                    "should-panic harness {name} may not draw direct symbolic input; use a checked constructor for the rejection domain"
                ));
            }
        }
        count += 1;
        i = end;
    }
    Ok(count)
}

fn walk(root: &Path) -> Audit<usize> {
    let mut entries = fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.path());
    let mut count = 0;
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!("source audit refuses symlink {}", path.display()));
        }
        if kind.is_dir() {
            count += walk(&path)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source =
                fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            count += audit(&source).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(count)
}

fn main() {
    let args: Vec<_> = env::args_os().collect();
    let result = if args.len() == 3 && args[1] == "--gate" {
        kani_gate::run(Path::new(&args[2]))
    } else if args.len() == 2 {
        walk(Path::new(&args[1])).map(|_| ())
    } else if args.len() == 4 && args[1] == "--inventory" {
        fs::read_to_string(&args[2])
            .map_err(|e| e.to_string())
            .and_then(|text| {
                let version = args[3].to_str().ok_or("non-UTF8 expected version")?;
                let total = kani_json::inventory_total(&text, version)?;
                println!("{total}");
                Ok(())
            })
    } else {
        eprintln!(
            "usage: check-kani-source SOURCE_DIRECTORY | --inventory JSON EXPECTED_VERSION | --gate ROOT"
        );
        process::exit(2);
    };
    if let Err(error) = result {
        eprintln!("Kani audit failed: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn harness(body: &str) -> String {
        format!("#[kani::proof]\n#[kani::should_panic]\nfn rejects() {{ {body} }}")
    }
    #[test]
    fn accepts_checked_production_calls() {
        assert_eq!(
            audit(&harness("let input = checked(); production(input);")),
            Ok(1)
        );
        assert_eq!(
            audit(
                "#[kani::should_panic] #[kani::unwind(2)] pub(crate) unsafe fn rejects() { production(); }"
            ),
            Ok(1)
        );
        assert_eq!(
            audit("#[kani :: should_panic] fn r#rejects() { production(); }"),
            Ok(1)
        );
    }
    #[test]
    fn rejects_every_direct_panic_family() {
        for name in [
            "assert",
            "assert_eq",
            "assert_ne",
            "debug_assert",
            "debug_assert_eq",
            "debug_assert_ne",
            "panic",
            "unreachable",
            "todo",
            "unimplemented",
        ] {
            for delimiters in [("(", ")"), ("[", "]"), ("{", "}")] {
                assert!(
                    audit(&harness(&format!(
                        "{name}!{}false{}; production();",
                        delimiters.0, delimiters.1
                    )))
                    .is_err(),
                    "accepted {name}"
                );
            }
        }
        for body in [
            "invariant(false);",
            "crate::invariant(false);",
            "kani::any::<u8>();",
            "kani /* gap */ :: any::<u8>();",
            "r#kani::r#any::<u8>();",
            "{ panic!(); }",
        ] {
            assert!(audit(&harness(body)).is_err(), "accepted {body}");
        }
    }
    #[test]
    fn rejects_unknown_and_truncated_harnesses() {
        for source in [
            "#[kani::should_panic] fn rejects() {",
            "#[kani::should_panic] fn rejects(x: u8) {}",
            "#[kani::should_panic] async fn rejects() {}",
            "#[kani::should_panic] fn rejects() -> () {}",
            "#[kani::should_panic] fn rejects() { (] }",
            "#[cfg_attr(kani, kani::should_panic)] fn rejects() {}",
            "#[kani::should_panic]",
            "/* unfinished",
            "\"unfinished",
            "r###\"unfinished",
        ] {
            assert!(audit(source).is_err(), "accepted {source}");
        }
    }
    #[test]
    fn ignores_comments_strings_chars_and_handles_lifetimes() {
        let body = r####"let _ = "} assert!(false)"; let _ = r###"} kani::any()"###; let _ = '}'; let _ = '\u{7d}'; let _ = '\x7d'; let _ = "escaped \" brace }"; /* } /* { */ panic!() */ // }
            production::<'static>();"####;
        assert_eq!(audit(&harness(body)), Ok(1));
        assert_eq!(
            audit(
                "// #[kani::should_panic] fn fake() { panic!(); }\nfn ordinary() { assert!(true); }"
            ),
            Ok(0)
        );
    }
}
