//! Stream CBMC JSON without diagnostic traces on Kani's reachability probes.
//! Property identities, statuses and real counterexamples pass through intact.
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::{
    env, fmt,
    io::{self, BufReader, BufWriter, Read, Write},
    process::{Command, Stdio},
};

#[derive(Default)]
struct Counts {
    properties: usize,
    traces: usize,
}

fn reachability_probe(fields: &Map<String, Value>) -> bool {
    let Some(id) = fields.get("property").and_then(Value::as_str) else {
        return false;
    };
    let Some((prefix, ordinal)) = id.rsplit_once('.') else {
        return false;
    };
    prefix.ends_with(".reachability_check")
        && !ordinal.is_empty()
        && ordinal.bytes().all(|b| b.is_ascii_digit())
        && fields
            .get("description")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with("KANI_CHECK_ID_"))
        && fields.get("sourceLocation").is_some_and(|location| {
            location.get("propertyClass").and_then(Value::as_str) == Some("reachability_check")
                && location.get("propertyId").and_then(Value::as_str) == Some(id)
        })
}

struct Property<'a>(&'a mut Counts);
impl<'de> DeserializeSeed<'de> for Property<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Property<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CBMC property")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
        let mut fields = Map::new();
        let mut trace_seen = false;
        while let Some(key) = input.next_key::<String>()? {
            if fields.contains_key(&key) || (key == "trace" && trace_seen) {
                return Err(de::Error::custom("duplicate CBMC property field"));
            }
            if key == "trace" {
                trace_seen = true;
                if !fields.contains_key("property") || !fields.contains_key("status") {
                    return Err(de::Error::custom(
                        "CBMC trace precedes property identity/status",
                    ));
                }
                if reachability_probe(&fields) {
                    input.next_value::<IgnoredAny>()?;
                    self.0.traces += 1;
                    continue;
                }
            }
            fields.insert(key, input.next_value::<Value>()?);
        }
        if !fields.get("property").is_some_and(Value::is_string)
            || !fields.get("status").is_some_and(Value::is_string)
        {
            return Err(de::Error::custom("missing CBMC property identity/status"));
        }
        self.0.properties += 1;
        Ok(Value::Object(fields))
    }
}

struct Properties<'a>(&'a mut Counts);
impl<'de> DeserializeSeed<'de> for Properties<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Properties<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CBMC property list")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
        let mut fields = Vec::new();
        while let Some(property) = input.next_element_seed(Property(self.0))? {
            fields.push(property);
        }
        if fields.is_empty() {
            return Err(de::Error::custom("empty CBMC property list"));
        }
        Ok(Value::Array(fields))
    }
}

struct Item<'a>(&'a mut Counts);
impl<'de> DeserializeSeed<'de> for Item<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Item<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CBMC output item")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
        let mut fields = Map::new();
        while let Some(key) = input.next_key::<String>()? {
            if fields.contains_key(&key) {
                return Err(de::Error::custom("duplicate CBMC item field"));
            }
            let value = if key == "result" {
                input.next_value_seed(Properties(self.0))?
            } else {
                input.next_value::<Value>()?
            };
            fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}

struct Output<'a, W>(&'a mut W, &'a mut Counts);
impl<'de, W: Write> Visitor<'de> for Output<'_, W> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CBMC output array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<(), A::Error> {
        self.0.write_all(b"[\n").map_err(de::Error::custom)?;
        let mut first = true;
        while let Some(item) = input.next_element_seed(Item(self.1))? {
            if !first {
                self.0.write_all(b",\n").map_err(de::Error::custom)?;
            }
            first = false;
            // Kani 0.67 recognizes top-level items by their two-space closing
            // brace. Preserve that framing while serializing one item at a time.
            let serialized = serde_json::to_string_pretty(&item).map_err(de::Error::custom)?;
            for (index, line) in serialized.lines().enumerate() {
                if index != 0 {
                    self.0.write_all(b"\n").map_err(de::Error::custom)?;
                }
                self.0.write_all(b"  ").map_err(de::Error::custom)?;
                self.0
                    .write_all(line.as_bytes())
                    .map_err(de::Error::custom)?;
            }
            self.0.flush().map_err(de::Error::custom)?;
        }
        if first {
            return Err(de::Error::custom("empty CBMC output"));
        }
        self.0.write_all(b"\n]\n").map_err(de::Error::custom)
    }
}

fn normalize(input: impl Read, output: &mut impl Write) -> Result<Counts, serde_json::Error> {
    let mut counts = Counts::default();
    let mut parser = serde_json::Deserializer::from_reader(input);
    de::Deserializer::deserialize_seq(&mut parser, Output(output, &mut counts))?;
    parser.end()?;
    output.flush().map_err(serde_json::Error::io)?;
    Ok(counts)
}

fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args == ["--normalize"] {
        let counts = normalize(
            BufReader::new(io::stdin().lock()),
            &mut BufWriter::new(io::stdout().lock()),
        )?;
        eprintln!(
            "CBMC JSON: properties={} diagnostic-traces-omitted={}",
            counts.properties, counts.traces
        );
        return Ok(0);
    }
    let real = env::var_os("HIBANA_REAL_CBMC").ok_or("missing HIBANA_REAL_CBMC")?;
    let mut command = Command::new(real);
    command.args(&args).stderr(Stdio::inherit());
    if !args.iter().any(|arg| arg == "--json-ui") {
        return Ok(command.status()?.code().unwrap_or(1));
    }
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let result = normalize(
        BufReader::new(child.stdout.take().ok_or("missing CBMC stdout")?),
        &mut BufWriter::new(io::stdout().lock()),
    );
    if let Err(error) = result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let counts = result?;
    eprintln!(
        "CBMC JSON: properties={} diagnostic-traces-omitted={}",
        counts.properties, counts.traces
    );
    let status = child.wait()?;
    if status.success() && counts.properties == 0 {
        return Err("CBMC returned no properties".into());
    }
    Ok(status.code().unwrap_or(1))
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("CBMC JSON diagnostics: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn probe(status: &str) -> Value {
        json!({"description":"KANI_CHECK_ID_test_0", "property":"test.reachability_check.1",
            "sourceLocation":{"propertyClass":"reachability_check","propertyId":"test.reachability_check.1"},
            "status":status,"trace":[{"stepType":"assignment","value":{"data":"large diagnostic"}}]})
    }

    #[test]
    fn only_diagnostic_traces_change_and_every_status_survives() {
        for status in ["FAILURE", "SUCCESS", "UNKNOWN", "UNDETERMINED"] {
            let mut input = json!([{"program":"CBMC"},{"result":[probe(status),
                {"property":"test.assertion.1","status":"FAILURE","trace":[{"data":"real counterexample"}]},
                {"property":"test.unwind.1","status":"SUCCESS"}]}]);
            let mut output = Vec::new();
            let counts = normalize(input.to_string().as_bytes(), &mut output).unwrap();
            assert_eq!((counts.properties, counts.traces), (3, 1));
            input[1]["result"][0]
                .as_object_mut()
                .unwrap()
                .remove("trace");
            assert_eq!(serde_json::from_slice::<Value>(&output).unwrap(), input);
            assert!(String::from_utf8(output).unwrap().contains("\n  },\n"));
        }
    }

    #[test]
    fn a_mislabelled_real_counterexample_is_kept() {
        let mut property = probe("FAILURE");
        property["sourceLocation"]["propertyId"] = json!("different.assertion.1");
        let input = json!([{"result":[property]}]);
        let mut output = Vec::new();
        assert_eq!(
            normalize(input.to_string().as_bytes(), &mut output)
                .unwrap()
                .traces,
            0
        );
        assert_eq!(serde_json::from_slice::<Value>(&output).unwrap(), input);
    }

    #[test]
    fn malformed_duplicate_and_reordered_identity_inputs_fail() {
        for input in [
            "[]",
            "[",
            "[{}] trailing",
            "[{\"result\":[]}]",
            "[{\"result\":[{\"property\":\"x\",\"property\":\"y\",\"status\":\"SUCCESS\"}]}]",
            "[{\"result\":[{\"trace\":[],\"property\":\"x\",\"status\":\"FAILURE\"}]}]",
        ] {
            assert!(
                normalize(input.as_bytes(), &mut Vec::new()).is_err(),
                "{input}"
            );
        }
    }
}
