#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunArgSource {
    Core,
    UserIdentity,
    WorkspaceVolume,
    CodexVolume,
    CargoVolume,
    SccacheVolume,
    SidecarNix,
    GuestDiagnostics,
    LibkrunOci,
    LibkrunMemory,
    LibkrunCpu,
    LibkrunNixDisk,
    LibkrunContainersDisk,
    LibkrunNetwork,
    LibkrunHostIdentity,
    LibkrunDebug,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunArgs {
    args: Vec<String>,
    sources: Vec<RunArgSource>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RunSpec {
    args: Vec<String>,
    sources: Vec<RunArgSource>,
}

impl RunSpec {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn arg(&mut self, source: RunArgSource, arg: impl Into<String>) {
        self.args.push(arg.into());
        self.sources.push(source);
    }

    pub(crate) fn args<I, S>(&mut self, source: RunArgSource, args: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for arg in args {
            self.arg(source, arg);
        }
    }

    pub(crate) fn option(
        &mut self,
        source: RunArgSource,
        flag: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.arg(source, flag);
        self.arg(source, value);
    }

    pub(crate) fn render(self) -> RunArgs {
        RunArgs {
            args: self.args,
            sources: self.sources,
        }
    }
}

impl RunArgs {
    pub(crate) fn into_vec(self) -> Vec<String> {
        self.args
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> &[String] {
        &self.args
    }

    #[cfg(test)]
    pub(crate) fn contains_arg_from(&self, source: RunArgSource, arg: &str) -> bool {
        self.args
            .iter()
            .zip(self.sources.iter())
            .any(|(candidate, candidate_source)| candidate == arg && *candidate_source == source)
    }

    #[cfg(test)]
    pub(crate) fn contains_option_from(
        &self,
        source: RunArgSource,
        flag: &str,
        value: &str,
    ) -> bool {
        self.args
            .windows(2)
            .zip(self.sources.windows(2))
            .any(|(args, sources)| {
                args[0] == flag && args[1] == value && sources[0] == source && sources[1] == source
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::podman::run::{RunArgSource, RunSpec};

    #[test]
    fn renderer_preserves_order_and_source_for_values() {
        let mut spec = RunSpec::new();
        spec.arg(RunArgSource::Core, "run");
        spec.option(RunArgSource::Core, "--name", "agentbox");
        spec.option(
            RunArgSource::WorkspaceVolume,
            "--volume",
            "/repo:/workspace",
        );
        spec.option(
            RunArgSource::GuestDiagnostics,
            "--env",
            "AGENTBOX_GUEST_DEBUG=1",
        );

        let rendered = spec.render();

        assert_eq!(
            rendered.as_slice(),
            [
                "run",
                "--name",
                "agentbox",
                "--volume",
                "/repo:/workspace",
                "--env",
                "AGENTBOX_GUEST_DEBUG=1"
            ]
        );
        assert!(rendered.contains_arg_from(RunArgSource::Core, "run"));
        assert!(rendered.contains_option_from(RunArgSource::Core, "--name", "agentbox"));
        assert!(rendered.contains_option_from(
            RunArgSource::WorkspaceVolume,
            "--volume",
            "/repo:/workspace"
        ));
        assert!(rendered.contains_option_from(
            RunArgSource::GuestDiagnostics,
            "--env",
            "AGENTBOX_GUEST_DEBUG=1"
        ));
    }
}
