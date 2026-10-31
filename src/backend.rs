use std::cell::RefCell;
use std::rc::Rc;

use qtbridge::{QApp, qobject};
use serde_json::{Value, json};

use crate::core::{Error, Vault, now};

struct Backend {
    vault: Vault,
    rows_json: String,
    error: String,
}

#[qobject(NoQmlElement)]
impl Backend {
    qproperty!("rowsJson", Member = rows_json, Notify = rows_json_changed);
    qproperty!("error", Member = error, Notify = error_changed);

    #[qsignal]
    fn rows_json_changed(&mut self);

    #[qsignal]
    fn error_changed(&mut self);

    #[qslot]
    fn refresh(&mut self) {
        self.rows_json = rows_json(&self.vault, now());
        self.rows_json_changed();
    }

    #[qslot]
    fn add(&mut self, name: String, key: String) {
        let result = self.vault.add(&name, &key);
        self.finish(result);
    }

    #[qslot]
    fn add_uri(&mut self, uri: String, name: String) {
        let name = Some(name.trim()).filter(|n| !n.is_empty());
        let result = self.vault.add_uri(&uri, name).map(drop);
        self.finish(result);
    }

    #[qslot]
    fn remove(&mut self, name: String) {
        let result = self.vault.remove(&name);
        self.finish(result);
    }
}

impl Backend {
    fn finish(&mut self, result: Result<(), Error>) {
        self.error = result.err().map(|e| e.to_string()).unwrap_or_default();
        self.error_changed();
    }
}

// Opens the window. Returns the exit code.
pub fn run(vault: Vault) -> i32 {
    let backend = Backend {
        vault,
        rows_json: String::new(),
        error: String::new(),
    };
    QApp::new()
        .set_initial_object("backend", Rc::new(RefCell::new(backend)))
        .load_qml(include_bytes!("Main.qml"))
        .run()
}

// The vault's keys for QML. `expires` is when the code stops being valid.
fn rows_json(vault: &Vault, unix: u64) -> String {
    vault
        .rows(unix)
        .into_iter()
        .map(|row| {
            // A broken key never "expires", or QML would ask for new codes forever.
            let remaining = if row.code.is_some() {
                row.remaining
            } else {
                row.period
            };
            json!({
                "name": row.name,
                "code": row.code.as_deref().unwrap_or_default(),
                "valid": row.code.is_some(),
                "expires": (unix + remaining) * 1000,
                "period": row.period,
            })
        })
        .collect::<Value>()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // "12345678901234567890" in base32. At t = 59 the code is 287082 (RFC 6238).
    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn rows(vault: &Vault, unix: u64) -> Vec<Value> {
        serde_json::from_str(&rows_json(vault, unix)).unwrap()
    }

    #[test]
    fn rows_carry_name_code_and_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path().join("keys.json")).unwrap();
        vault.add("a", SECRET).unwrap();

        let rows = rows(&vault, 59);

        assert_eq!(
            rows,
            [
                json!({ "name": "a", "code": "287082", "valid": true, "expires": 60_000, "period": 30 })
            ]
        );
    }

    #[test]
    fn rows_keep_the_vault_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path().join("keys.json")).unwrap();
        for name in ["b", "a", "c"] {
            vault.add(name, SECRET).unwrap();
        }

        let names: Vec<_> = rows(&vault, 0).iter().map(|r| r["name"].clone()).collect();

        assert_eq!(names, ["b", "a", "c"]);
    }

    #[test]
    fn a_broken_key_is_invalid_and_does_not_expire_now() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        std::fs::write(&path, r#"{"bad":"not base32!"}"#).unwrap();
        let vault = Vault::open(path).unwrap();

        let rows = rows(&vault, 100);

        assert_eq!(rows[0]["valid"], false);
        assert_eq!(rows[0]["code"], "");
        assert!(rows[0]["expires"].as_u64().unwrap() > 100_000);
    }

    #[test]
    fn an_empty_vault_is_an_empty_array() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path().join("keys.json")).unwrap();

        assert_eq!(rows_json(&vault, 0), "[]");
    }
}
