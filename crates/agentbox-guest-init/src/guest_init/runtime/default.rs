use anyhow::{Context, Result};

use crate::guest_init::cli::{DefaultCommand, DefaultSubcommand};
use crate::guest_init::process;

const LIBKRUN_NIX_OVERLAY_ENV: &str = "AGENTBOX_LIBKRUN_NIX_OVERLAY";
const LIBKRUN_CONTAINERS_STORAGE_ENV: &str = "AGENTBOX_LIBKRUN_CONTAINERS_STORAGE";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::guest_init) enum DefaultEnterOperation {
    ResolveCommand,
    DispatchLibkrunIfRequested,
    DispatchContainer,
}

#[cfg(test)]
pub(in crate::guest_init) fn planned_enter_operations() -> Vec<DefaultEnterOperation> {
    vec![
        DefaultEnterOperation::ResolveCommand,
        DefaultEnterOperation::DispatchLibkrunIfRequested,
        DefaultEnterOperation::DispatchContainer,
    ]
}

pub(in crate::guest_init) fn run(command: DefaultCommand) -> Result<()> {
    match command.command {
        DefaultSubcommand::Enter(enter_command) => {
            let command = enter_command.resolved_command();
            if should_dispatch_libkrun_from_env() {
                process::exec_command(&libkrun_dispatch_argv(&command)?)
            } else {
                process::exec_command(&container_dispatch_argv(&command)?)
            }
        }
    }
}

fn should_dispatch_libkrun_from_env() -> bool {
    should_dispatch_libkrun(&ProcessEnv)
}

fn should_dispatch_libkrun(env: &impl EnvSource) -> bool {
    env.var(LIBKRUN_NIX_OVERLAY_ENV).as_deref() == Some("1")
        || env.var(LIBKRUN_CONTAINERS_STORAGE_ENV).as_deref() == Some("1")
}

fn libkrun_dispatch_argv(command: &[String]) -> Result<Vec<String>> {
    Ok(libkrun_dispatch_argv_for_exe(&current_exe()?, command))
}

pub(in crate::guest_init) fn libkrun_dispatch_argv_for_exe(
    exe: &str,
    command: &[String],
) -> Vec<String> {
    runtime_dispatch_argv_for_exe(exe, "libkrun", command)
}

fn container_dispatch_argv(command: &[String]) -> Result<Vec<String>> {
    Ok(container_dispatch_argv_for_exe(&current_exe()?, command))
}

pub(in crate::guest_init) fn container_dispatch_argv_for_exe(
    exe: &str,
    command: &[String],
) -> Vec<String> {
    runtime_dispatch_argv_for_exe(exe, "container", command)
}

fn runtime_dispatch_argv_for_exe(exe: &str, runtime: &str, command: &[String]) -> Vec<String> {
    let mut argv = vec![
        exe.to_owned(),
        runtime.to_owned(),
        "enter".to_owned(),
        "--".to_owned(),
    ];
    argv.extend(command.iter().cloned());
    argv
}

fn current_exe() -> Result<String> {
    Ok(std::env::current_exe()
        .context("failed to resolve current agentbox-guest-init executable")?
        .display()
        .to_string())
}

trait EnvSource {
    fn var(&self, name: &str) -> Option<String>;
}

struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }
}

#[cfg(test)]
#[path = "default_tests.rs"]
mod tests;
