use anyhow::{Result, anyhow};
use color_eyre::eyre::Result as EyreResult;
use forksync_config::AgentProvider;
use inquire::Confirm;
use interactive_clap::ResultFromCli;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPushPreflight {
    pub safe_to_push_main: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitWizardState {
    CheckPushSafety,
    ConfirmAutoPush,
    SelectAgent { auto_push_managed_refs: bool },
    Finished,
}

impl InitWizardState {
    pub fn after_preflight(preflight: &InitPushPreflight) -> Self {
        if preflight.safe_to_push_main {
            Self::ConfirmAutoPush
        } else {
            Self::SelectAgent {
                auto_push_managed_refs: false,
            }
        }
    }

    pub fn after_auto_push_choice(self, allow_auto_push: bool) -> Self {
        match self {
            Self::ConfirmAutoPush => Self::SelectAgent {
                auto_push_managed_refs: allow_auto_push,
            },
            other => other,
        }
    }

    pub fn finish(self) -> Self {
        Self::Finished
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitWizardDecisions {
    pub auto_push: bool,
    pub agent_provider: AgentProvider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInitPreferences {
    pub auto_push: bool,
    pub agent_provider: AgentProvider,
    pub used_wizard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitWizardAgentChoice {
    OpenCode,
    Disabled,
}

impl InitWizardAgentChoice {
    fn into_agent_provider(self) -> AgentProvider {
        match self {
            Self::OpenCode => AgentProvider::OpenCode,
            Self::Disabled => AgentProvider::Disabled,
        }
    }
}

#[derive(Debug, Clone, Default, interactive_clap::InteractiveClap)]
#[interactive_clap(input_context = InitPushPreflight)]
pub struct InitWizardPrompt {
    /// Let ForkSync publish the managed branches to `main` for you when that push looks safe.
    #[interactive_clap(skip_default_input_arg)]
    auto_push: bool,
    #[interactive_clap(subcommand)]
    agent: InitWizardAgentPrompt,
}

#[derive(Debug, Clone, Default, interactive_clap::InteractiveClap)]
pub enum InitWizardAgentPrompt {
    /// Keep OpenCode enabled with the default model `opencode/gpt-5-nano`.
    #[default]
    OpenCode,
    /// Disable AI repair.
    Disabled,
}

impl InitWizardPrompt {
    fn input_auto_push(context: &InitPushPreflight) -> EyreResult<Option<bool>> {
        if !context.safe_to_push_main {
            if let Some(reason) = &context.reason {
                println!("ForkSync will skip automatic push to `main`: {reason}");
            } else {
                println!(
                    "ForkSync will skip automatic push to `main` because push safety could not be confirmed."
                );
            }
            return Ok(Some(false));
        }

        Confirm::new("ForkSync can publish the managed branches to `main` for you. Let it push now?")
            .with_default(true)
            .prompt()
            .map(Some)
            .map_err(Into::into)
    }
}

pub fn run_init_wizard(preflight: InitPushPreflight) -> Result<InitWizardDecisions> {
    println!(
        "ForkSync defaults to OpenCode with model `opencode/gpt-5-nano`. You can keep that default or disable AI repair."
    );

    let mut prompt = InitWizardPrompt::default();
    loop {
        match <InitWizardPrompt as interactive_clap::FromCli>::from_cli(
            Some(prompt.clone()),
            preflight.clone(),
        ) {
            ResultFromCli::Ok(prompt) | ResultFromCli::Cancel(Some(prompt)) => {
                return Ok(map_prompt_to_decisions(prompt));
            }
            ResultFromCli::Cancel(None) => return Err(anyhow!("interactive init canceled")),
            ResultFromCli::Back => {
                prompt = InitWizardPrompt::default();
            }
            ResultFromCli::Err(_, err) => return Err(err.into()),
        }
    }
}

fn map_prompt_to_decisions(prompt: InitWizardPrompt) -> InitWizardDecisions {
    let agent_provider = match prompt.agent {
        InitWizardAgentPrompt::OpenCode => InitWizardAgentChoice::OpenCode,
        InitWizardAgentPrompt::Disabled => InitWizardAgentChoice::Disabled,
    }
    .into_agent_provider();

    InitWizardDecisions {
        auto_push: prompt.auto_push,
        agent_provider,
    }
}

pub fn resolve_init_preferences(
    preflight: &InitPushPreflight,
    should_run_wizard: bool,
    manual_push_output_flag: bool,
    agent_provider_flag: Option<AgentProvider>,
    wizard_decisions: Option<InitWizardDecisions>,
) -> ResolvedInitPreferences {
    if should_run_wizard {
        let wizard_decisions =
            wizard_decisions.expect("wizard decisions are required when the wizard runs");
        return ResolvedInitPreferences {
            auto_push: wizard_decisions.auto_push,
            agent_provider: wizard_decisions.agent_provider,
            used_wizard: true,
        };
    }

    let state = InitWizardState::CheckPushSafety;
    let state = match state {
        InitWizardState::CheckPushSafety => InitWizardState::after_preflight(preflight),
        other => other,
    };

    let auto_push = match state {
        InitWizardState::ConfirmAutoPush => !manual_push_output_flag,
        InitWizardState::SelectAgent {
            auto_push_managed_refs,
        } => auto_push_managed_refs,
        InitWizardState::CheckPushSafety | InitWizardState::Finished => false,
    };
    let _ = state.after_auto_push_choice(auto_push).finish();

    ResolvedInitPreferences {
        auto_push,
        agent_provider: agent_provider_flag.unwrap_or(AgentProvider::OpenCode),
        used_wizard: false,
    }
}

pub fn should_run_init_wizard(
    non_interactive: bool,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    manual_push_output_flag: bool,
    agent_provider_flag: Option<AgentProvider>,
) -> bool {
    !non_interactive
        && stdin_is_terminal
        && stdout_is_terminal
        && !manual_push_output_flag
        && agent_provider_flag.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_skips_auto_push_confirmation_when_preflight_is_unsafe() {
        let next = InitWizardState::after_preflight(&InitPushPreflight {
            safe_to_push_main: false,
            reason: Some("branch protection rejected dry-run push".to_string()),
        });

        assert_eq!(
            next,
            InitWizardState::SelectAgent {
                auto_push_managed_refs: false
            }
        );
    }

    #[test]
    fn non_interactive_defaults_use_safe_push_and_opencode() {
        let resolved = resolve_init_preferences(
            &InitPushPreflight {
                safe_to_push_main: true,
                reason: None,
            },
            false,
            false,
            None,
            None,
        );

        assert_eq!(
            resolved,
            ResolvedInitPreferences {
                auto_push: true,
                agent_provider: AgentProvider::OpenCode,
                used_wizard: false,
            }
        );
    }

    #[test]
    fn wizard_decisions_override_non_interactive_defaults() {
        let resolved = resolve_init_preferences(
            &InitPushPreflight {
                safe_to_push_main: true,
                reason: None,
            },
            true,
            false,
            None,
            Some(InitWizardDecisions {
                auto_push: false,
                agent_provider: AgentProvider::Disabled,
            }),
        );

        assert_eq!(
            resolved,
            ResolvedInitPreferences {
                auto_push: false,
                agent_provider: AgentProvider::Disabled,
                used_wizard: true,
            }
        );
    }

    #[test]
    fn combinatorial_resolution_matches_the_init_rules() {
        let safe_values = [true, false];
        let wizard_values = [true, false];
        let manual_values = [true, false];
        let explicit_agents = [None, Some(AgentProvider::OpenCode), Some(AgentProvider::Disabled)];
        let wizard_agents = [AgentProvider::OpenCode, AgentProvider::Disabled];
        let wizard_push_values = [true, false];

        for safe_to_push_main in safe_values {
            for should_run_wizard in wizard_values {
                for manual_push_output_flag in manual_values {
                    for explicit_agent_provider in explicit_agents {
                        if should_run_wizard
                            && (manual_push_output_flag || explicit_agent_provider.is_some())
                        {
                            continue;
                        }

                        for wizard_agent_provider in wizard_agents {
                            for wizard_auto_push in wizard_push_values {
                                let wizard_decisions = should_run_wizard.then_some(
                                    InitWizardDecisions {
                                        auto_push: wizard_auto_push,
                                        agent_provider: wizard_agent_provider,
                                    },
                                );

                                let resolved = resolve_init_preferences(
                                    &InitPushPreflight {
                                        safe_to_push_main,
                                        reason: None,
                                    },
                                    should_run_wizard,
                                    manual_push_output_flag,
                                    explicit_agent_provider,
                                    wizard_decisions,
                                );

                                if should_run_wizard {
                                    assert!(resolved.used_wizard);
                                    assert_eq!(resolved.auto_push, wizard_auto_push);
                                    assert_eq!(resolved.agent_provider, wizard_agent_provider);
                                } else {
                                    assert!(!resolved.used_wizard);
                                    assert_eq!(
                                        resolved.auto_push,
                                        !manual_push_output_flag && safe_to_push_main
                                    );
                                    assert_eq!(
                                        resolved.agent_provider,
                                        explicit_agent_provider.unwrap_or(AgentProvider::OpenCode)
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wizard_runs_only_when_terminals_are_present_and_flags_do_not_override_it() {
        assert!(should_run_init_wizard(false, true, true, false, None));
        assert!(!should_run_init_wizard(true, true, true, false, None));
        assert!(!should_run_init_wizard(false, false, true, false, None));
        assert!(!should_run_init_wizard(false, true, false, false, None));
        assert!(!should_run_init_wizard(false, true, true, true, None));
        assert!(!should_run_init_wizard(
            false,
            true,
            true,
            false,
            Some(AgentProvider::Disabled)
        ));
    }
}
