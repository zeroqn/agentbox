//! Network-mode contribution to guest bootstrap environment.

use std::collections::BTreeMap;

use super::super::guest_env;
use super::super::model::{GUEST_USE_PASST_ENV, NetworkMode};

pub(crate) fn contribute_guest_env(env: &mut BTreeMap<String, String>, mode: NetworkMode) {
    if mode == NetworkMode::Passt {
        guest_env::insert_env(env, GUEST_USE_PASST_ENV, "1");
    }
}
