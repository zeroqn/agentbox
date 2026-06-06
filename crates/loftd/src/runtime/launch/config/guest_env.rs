//! Guest bootstrap environment and guest config JSON ownership.

use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

use super::model::{IMAGE_LOFTD_ENV_ALLOWLIST, IMAGE_PATH_ENV};

pub(crate) fn bootstrap_env(
    image_env: &[String],
    required_env: Vec<(String, String)>,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for entry in image_env {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow!("loftd image env entry '{entry}' is missing '='"))?;
        if key.is_empty() {
            anyhow::bail!("loftd image env entry '{entry}' has an empty key");
        }
        if is_allowed_image_env(key) {
            env.insert(key.to_owned(), value.to_owned());
        }
    }
    for (key, value) in required_env {
        env.insert(key, value);
    }
    Ok(env)
}

fn is_allowed_image_env(key: &str) -> bool {
    key == IMAGE_PATH_ENV || IMAGE_LOFTD_ENV_ALLOWLIST.contains(&key)
}

pub(crate) fn insert_env(env: &mut BTreeMap<String, String>, key: &str, value: &str) {
    env.insert(key.to_owned(), value.to_owned());
}

pub(crate) fn guest_config_json(env: &[(String, String)]) -> String {
    let mut out = String::from("{\n  \"Env\": [");
    for (index, (key, value)) in env.iter().enumerate() {
        if index == 0 {
            out.push('\n');
        } else {
            out.push_str(",\n");
        }
        out.push_str("    \"");
        push_json_escaped(&mut out, key);
        out.push('=');
        push_json_escaped(&mut out, value);
        out.push('"');
    }
    if env.is_empty() {
        out.push_str("]\n}\n");
    } else {
        out.push_str("\n  ]\n}\n");
    }
    out
}

fn push_json_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
}
