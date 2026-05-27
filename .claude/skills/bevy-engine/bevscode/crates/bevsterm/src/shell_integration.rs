use bevy::prelude::*;

use crate::backend::SharedWriter;

/// Injects shell integration hooks (e.g. OSC 133) into a freshly-opened PTY.
pub trait ShellIntegration: Send + Sync {
    fn inject(&self, pty_input: &SharedWriter);
}

/// Per-entity shell integration. Insert alongside `BevyTerminal` to enable
/// OSC 133 command blocks.
#[derive(Component)]
pub struct ShellIntegrationComponent(pub Box<dyn ShellIntegration>);

impl ShellIntegrationComponent {
    pub fn new(integration: impl ShellIntegration + 'static) -> Self {
        Self(Box::new(integration))
    }

    pub fn auto() -> Self {
        Self(auto_detect(None))
    }
}

pub struct NoIntegration;

impl ShellIntegration for NoIntegration {
    fn inject(&self, _: &SharedWriter) {}
}

pub struct ZshIntegration;

impl ShellIntegration for ZshIntegration {
    fn inject(&self, pty_input: &SharedWriter) {
        let script = r#"__bevsterm_precmd() { print -Pn "\e]133;A\a"; }
__bevsterm_preexec() { print -Pn "\e]133;C\a"; }
typeset -ag precmd_functions preexec_functions
precmd_functions+=(__bevsterm_precmd)
preexec_functions+=(__bevsterm_preexec)
export BEVSTERM_BOOTSTRAPPED=1"#;
        let cmd = format!(
            " [[ -z $BEVSTERM_BOOTSTRAPPED ]] && eval '{}'\n",
            script.replace('\'', "'\\''")
        );
        let _ = pty_input.write_bytes(cmd.as_bytes());
    }
}

pub struct BashIntegration;

impl ShellIntegration for BashIntegration {
    fn inject(&self, pty_input: &SharedWriter) {
        let script = r#"__bevsterm_prompt_command() { printf '\e]133;A\a'; }
__bevsterm_preexec() {
    if [[ "$BASH_COMMAND" != "$PROMPT_COMMAND" ]]; then
        printf '\e]133;C\a'
    fi
}
PROMPT_COMMAND="__bevsterm_prompt_command;${PROMPT_COMMAND:-:}"
trap '__bevsterm_preexec' DEBUG
export BEVSTERM_BOOTSTRAPPED=1"#;
        let cmd = format!(
            " [[ -z $BEVSTERM_BOOTSTRAPPED ]] && eval '{}'\n",
            script.replace('\'', "'\\''")
        );
        let _ = pty_input.write_bytes(cmd.as_bytes());
    }
}

pub fn auto_detect(shell_program: Option<&str>) -> Box<dyn ShellIntegration> {
    let shell = shell_program
        .map(|s| s.to_string())
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_default();
    let name = shell
        .rsplit('/')
        .next()
        .unwrap_or(&shell)
        .trim_start_matches('-'); // login shells prefix with '-'

    match name {
        "zsh" => Box::new(ZshIntegration),
        "bash" => Box::new(BashIntegration),
        _ => Box::new(NoIntegration),
    }
}
