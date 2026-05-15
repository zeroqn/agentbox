#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunArgOwner(&'static str);

impl RunArgOwner {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self(name)
    }
}

pub(crate) const CORE: RunArgOwner = RunArgOwner::new("podman.core");

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunArgs {
    args: Vec<String>,
    owners: Vec<RunArgOwner>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RunSpec {
    args: Vec<String>,
    owners: Vec<RunArgOwner>,
}

impl RunSpec {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn arg(&mut self, owner: RunArgOwner, arg: impl Into<String>) {
        self.args.push(arg.into());
        self.owners.push(owner);
    }

    pub(crate) fn args<I, S>(&mut self, owner: RunArgOwner, args: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for arg in args {
            self.arg(owner, arg);
        }
    }

    pub(crate) fn option(
        &mut self,
        owner: RunArgOwner,
        flag: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.arg(owner, flag);
        self.arg(owner, value);
    }

    pub(crate) fn render(self) -> RunArgs {
        RunArgs {
            args: self.args,
            owners: self.owners,
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
    pub(crate) fn contains_arg_from(&self, owner: RunArgOwner, arg: &str) -> bool {
        self.args
            .iter()
            .zip(self.owners.iter())
            .any(|(candidate, candidate_owner)| candidate == arg && *candidate_owner == owner)
    }

    #[cfg(test)]
    pub(crate) fn contains_option_from(&self, owner: RunArgOwner, flag: &str, value: &str) -> bool {
        self.args
            .windows(2)
            .zip(self.owners.windows(2))
            .any(|(args, owners)| {
                args[0] == flag && args[1] == value && owners[0] == owner && owners[1] == owner
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::podman::run::{CORE, RunArgOwner, RunSpec};

    const TEST_COMPONENT_A_OWNER: RunArgOwner = RunArgOwner::new("test.component_a");
    const TEST_COMPONENT_B_OWNER: RunArgOwner = RunArgOwner::new("test.component_b");

    #[test]
    fn renderer_preserves_order_and_owner_for_values() {
        let mut spec = RunSpec::new();
        spec.arg(CORE, "run");
        spec.option(CORE, "--name", "agentbox");
        spec.option(TEST_COMPONENT_A_OWNER, "--component-a", "alpha");
        spec.option(TEST_COMPONENT_B_OWNER, "--component-b", "beta");

        let rendered = spec.render();

        assert_eq!(
            rendered.as_slice(),
            [
                "run",
                "--name",
                "agentbox",
                "--component-a",
                "alpha",
                "--component-b",
                "beta"
            ]
        );
        assert!(rendered.contains_arg_from(CORE, "run"));
        assert!(rendered.contains_option_from(CORE, "--name", "agentbox"));
        assert!(rendered.contains_option_from(TEST_COMPONENT_A_OWNER, "--component-a", "alpha"));
        assert!(rendered.contains_option_from(TEST_COMPONENT_B_OWNER, "--component-b", "beta"));
    }
}
