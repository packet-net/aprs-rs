//! Comparing a produced value with an expected one by the vectors' rules. Shared by the vector
//! runner and the differential dump (examples/diff_dump.rs).

use serde_json::Value;

/// Differences between an expected and a produced value, by the vectors' rules: exact key sets,
/// strings, booleans and integers exactly, other numbers within 1e-9.
pub fn differences(expected: &Value, actual: &Value, at: &str, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, ev) in e {
                match a.get(k) {
                    Some(av) => differences(ev, av, &format!("{at}.{k}"), out),
                    None => out.push(format!("{at}.{k}: expected {ev}, missing")),
                }
            }
            for (k, av) in a {
                if !e.contains_key(k) {
                    out.push(format!("{at}.{k}: unexpected {av}"));
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                out.push(format!("{at}: expected {expected}, got {actual}"));
                return;
            }
            for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                differences(ev, av, &format!("{at}[{i}]"), out);
            }
        }
        (Value::Number(e), Value::Number(a)) => {
            let same = match (e.as_i64(), a.as_i64(), e.is_f64() || a.is_f64()) {
                (Some(x), Some(y), false) => x == y,
                _ => {
                    let (x, y) = (e.as_f64().unwrap(), a.as_f64().unwrap());
                    let scale = x.abs().max(y.abs());
                    (x - y).abs() <= 1e-9 * if scale < 1.0 { 1.0 } else { scale }
                }
            };
            if !same {
                out.push(format!("{at}: expected {e}, got {a}"));
            }
        }
        _ => {
            if expected != actual {
                out.push(format!("{at}: expected {expected}, got {actual}"));
            }
        }
    }
}
