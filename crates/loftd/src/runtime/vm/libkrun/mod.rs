//! libkrun VM launch integration.

mod api;
mod dynamic;
mod launcher;

#[cfg(test)]
pub(in crate::runtime::vm::libkrun) use api::LibkrunApi;
pub(in crate::runtime) use dynamic::DynamicLibkrunApi;
pub(in crate::runtime) use launcher::DirectLibkrunLauncher;
pub(in crate::runtime) use launcher::RENDER_SERVER_FD_ENV;

#[cfg(test)]
use dynamic::{
    LOFTD_LIBKRUN_COMPAT_NET_FEATURES, nested_virt_symbol_presence_for_test,
    planned_libkrun_load_order, planned_libkrun_load_order_for_exe,
    required_rlimits_symbol_presence_for_test,
};

#[cfg(test)]
mod tests;
