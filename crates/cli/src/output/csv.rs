//! Data as CSV: a list is a row per item, anything else one row. An object gives dotted
//! columns such as pose.x, a list inside a row is one cell of compact JSON.

use serde_json::Value;

/// The header and every row, no newline at the end. Columns come in the order rows first
/// have them, and a row without one leaves its cell empty.
pub fn table(v: &Value) -> String {
    let rows: Vec<Vec<(String, &Value)>> = match v {
        Value::Array(items) => items.iter().map(cells).collect(),
        v => vec![cells(v)],
    };
    let mut columns: Vec<&str> = Vec::new();
    for (column, _) in rows.iter().flatten() {
        if !columns.contains(&column.as_str()) {
            columns.push(column);
        }
    }
    let mut lines = vec![columns.iter().map(|c| field(c)).collect::<Vec<_>>().join(",")];
    for row in &rows {
        let cell = |c: &&str| row.iter().find(|(name, _)| name == c).map(|(_, v)| text(v)).unwrap_or_default();
        lines.push(columns.iter().map(cell).collect::<Vec<_>>().join(","));
    }
    lines.join("\n")
}

/// One row's cells by column, `value` for a row that is no object.
fn cells(v: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    flatten(v, "", &mut out);
    out
}

fn flatten<'a>(v: &'a Value, path: &str, out: &mut Vec<(String, &'a Value)>) {
    match v {
        Value::Object(map) => {
            for (k, e) in map {
                flatten(e, &if path.is_empty() { k.clone() } else { format!("{path}.{k}") }, out);
            }
        }
        v => out.push((if path.is_empty() { "value".to_string() } else { path.to_string() }, v)),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => field(s),
        v => field(&v.to_string()),
    }
}

/// RFC 4180: a field holding a comma, a quote or a line break is quoted, quotes doubled.
fn field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_object_is_one_row_with_dotted_columns() {
        let v = json!({"seq": 1, "pose": {"x": 0.5}, "pts": [{"y": 2}], "name": "a,b", "say": "\"hi\"", "none": null});
        assert_eq!(table(&v), "seq,pose.x,pts,name,say,none\n1,0.5,\"[{\"\"y\"\":2}]\",\"a,b\",\"\"\"hi\"\"\",");
    }

    #[test]
    fn a_list_is_a_row_per_item() {
        let v = json!([{"name": "a", "pid": 1}, {"name": "b", "detail": "gone"}]);
        assert_eq!(table(&v), "name,pid,detail\na,1,\nb,,gone");
        assert_eq!(table(&json!(["x", "y"])), "value\nx\ny");
    }

    #[test]
    fn a_bare_value_is_its_own_column() {
        assert_eq!(table(&json!(1.5)), "value\n1.5");
    }
}
