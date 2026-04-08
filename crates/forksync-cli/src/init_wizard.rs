use anyhow::{Context, Result, anyhow};
use cliclack::{confirm, intro, note, outro, outro_cancel, select};
use forksync_config::AgentProvider;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPushPreflight {
    pub output_branch: String,
    pub safe_to_push_output: bool,
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
        if preflight.safe_to_push_output {
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
    pub publish_to_registry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInitPreferences {
    pub auto_push: bool,
    pub agent_provider: AgentProvider,
    pub publish_to_registry: bool,
    pub used_wizard: bool,
}

pub fn run_init_wizard(
    preflight: InitPushPreflight,
    offer_registry_publish: bool,
) -> Result<InitWizardDecisions> {
    intro("ForkSync init").context("start cliclack init session")?;
    let state = InitWizardState::after_preflight(&preflight);

    let auto_push = match state {
        InitWizardState::ConfirmAutoPush => prompt_auto_push_choice()?,
        InitWizardState::SelectAgent {
            auto_push_managed_refs,
        } => {
            if let Some(reason) = &preflight.reason {
                note(
                    "Automatic push disabled",
                    format!(
                        "ForkSync will skip direct publication to `{}`: {reason}",
                        preflight.output_branch
                    ),
                )
                .context("render cliclack push note")?;
            }
            auto_push_managed_refs
        }
        InitWizardState::CheckPushSafety | InitWizardState::Finished => false,
    };

    note(
        "Agent default",
        "ForkSync defaults to OpenCode with model `opencode/gpt-5-nano`. You can keep that default or disable AI repair for a deterministic-only setup.",
    )
    .context("render cliclack agent note")?;
    let agent_provider = prompt_agent_choice()?;
    let publish_to_registry = if offer_registry_publish {
        note(
            "Optional registry publish",
            "If this fork is public, ForkSync can publish its source metadata to the public registry later.",
        )
        .context("render cliclack registry note")?;
        prompt_registry_publish_choice()?
    } else {
        false
    };
    let _ = state.after_auto_push_choice(auto_push).finish();
    outro("Captured init preferences. Applying bootstrap plan...")?;

    Ok(InitWizardDecisions {
        auto_push,
        agent_provider,
        publish_to_registry,
    })
}

fn prompt_auto_push_choice() -> Result<bool> {
    confirm(
        "ForkSync can publish the managed branches to the output branch for you. Let it push now?",
    )
    .initial_value(true)
    .interact()
    .map_err(|err| map_prompt_error(err, "auto-push confirmation"))
}

fn prompt_agent_choice() -> Result<AgentProvider> {
    select("Choose the agent mode for conflict repair")
        .item(
            AgentProvider::OpenCode,
            "OpenCode",
            "default: opencode/gpt-5-nano",
        )
        .item(AgentProvider::Disabled, "No AI repair", "")
        .initial_value(AgentProvider::OpenCode)
        .interact()
        .map_err(|err| map_prompt_error(err, "agent selection"))
}

fn prompt_registry_publish_choice() -> Result<bool> {
    confirm("Publish this fork to the public ForkSync registry if it is public?")
        .initial_value(false)
        .interact()
        .map_err(|err| map_prompt_error(err, "registry publish confirmation"))
}

fn map_prompt_error(err: io::Error, prompt_name: &str) -> anyhow::Error {
    if err.kind() == io::ErrorKind::Interrupted {
        let _ = outro_cancel("Init setup cancelled.");
        anyhow!("interactive init canceled")
    } else {
        anyhow!("interactive {prompt_name} failed: {err}")
    }
}

pub fn resolve_init_preferences(
    preflight: &InitPushPreflight,
    should_run_wizard: bool,
    manual_push_output_flag: bool,
    agent_provider_flag: Option<AgentProvider>,
    publish_to_registry_flag: bool,
    wizard_decisions: Option<InitWizardDecisions>,
) -> ResolvedInitPreferences {
    if should_run_wizard {
        let wizard_decisions =
            wizard_decisions.expect("wizard decisions are required when the wizard runs");
        return ResolvedInitPreferences {
            auto_push: wizard_decisions.auto_push,
            agent_provider: wizard_decisions.agent_provider,
            publish_to_registry: wizard_decisions.publish_to_registry,
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
        publish_to_registry: publish_to_registry_flag,
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
            output_branch: "main".to_string(),
            safe_to_push_output: false,
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
    fn state_machine_prompts_for_auto_push_when_preflight_is_safe() {
        let next = InitWizardState::after_preflight(&InitPushPreflight {
            output_branch: "main".to_string(),
            safe_to_push_output: true,
            reason: None,
        });

        assert_eq!(next, InitWizardState::ConfirmAutoPush);
        assert_eq!(
            next.after_auto_push_choice(false),
            InitWizardState::SelectAgent {
                auto_push_managed_refs: false
            }
        );
    }

    #[test]
    fn non_interactive_defaults_use_safe_push_and_opencode() {
        let resolved = resolve_init_preferences(
            &InitPushPreflight {
                output_branch: "main".to_string(),
                safe_to_push_output: true,
                reason: None,
            },
            false,
            false,
            None,
            false,
            None,
        );

        assert_eq!(
            resolved,
            ResolvedInitPreferences {
                auto_push: true,
                agent_provider: AgentProvider::OpenCode,
                publish_to_registry: false,
                used_wizard: false,
            }
        );
    }

    #[test]
    fn unsafe_preflight_forces_manual_publication_even_with_explicit_auto_push_intent() {
        let resolved = resolve_init_preferences(
            &InitPushPreflight {
                output_branch: "main".to_string(),
                safe_to_push_output: false,
                reason: Some("branch protection rejected the dry-run push".to_string()),
            },
            false,
            false,
            Some(AgentProvider::OpenCode),
            false,
            None,
        );

        assert_eq!(
            resolved,
            ResolvedInitPreferences {
                auto_push: false,
                agent_provider: AgentProvider::OpenCode,
                publish_to_registry: false,
                used_wizard: false,
            }
        );
    }

    #[test]
    fn wizard_decisions_override_non_interactive_defaults() {
        let resolved = resolve_init_preferences(
            &InitPushPreflight {
                output_branch: "main".to_string(),
                safe_to_push_output: true,
                reason: None,
            },
            true,
            false,
            None,
            false,
            Some(InitWizardDecisions {
                auto_push: false,
                agent_provider: AgentProvider::Disabled,
                publish_to_registry: true,
            }),
        );

        assert_eq!(
            resolved,
            ResolvedInitPreferences {
                auto_push: false,
                agent_provider: AgentProvider::Disabled,
                publish_to_registry: true,
                used_wizard: true,
            }
        );
    }

    #[test]
    fn combinatorial_resolution_matches_the_init_rules() {
        let safe_values = [true, false];
        let wizard_values = [true, false];
        let manual_values = [true, false];
        let explicit_agents = [
            None,
            Some(AgentProvider::OpenCode),
            Some(AgentProvider::Disabled),
        ];
        let wizard_agents = [AgentProvider::OpenCode, AgentProvider::Disabled];
        let wizard_push_values = [true, false];
        let registry_values = [true, false];

        for safe_to_push_output in safe_values {
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
                                for publish_to_registry in registry_values {
                                    let wizard_decisions =
                                        should_run_wizard.then_some(InitWizardDecisions {
                                            auto_push: wizard_auto_push,
                                            agent_provider: wizard_agent_provider,
                                            publish_to_registry,
                                        });

                                    let resolved = resolve_init_preferences(
                                        &InitPushPreflight {
                                            output_branch: "main".to_string(),
                                            safe_to_push_output,
                                            reason: None,
                                        },
                                        should_run_wizard,
                                        manual_push_output_flag,
                                        explicit_agent_provider,
                                        publish_to_registry,
                                        wizard_decisions,
                                    );

                                    if should_run_wizard {
                                        assert!(resolved.used_wizard);
                                        assert_eq!(resolved.auto_push, wizard_auto_push);
                                        assert_eq!(resolved.agent_provider, wizard_agent_provider);
                                        assert_eq!(
                                            resolved.publish_to_registry,
                                            publish_to_registry
                                        );
                                    } else {
                                        assert!(!resolved.used_wizard);
                                        assert_eq!(
                                            resolved.auto_push,
                                            !manual_push_output_flag && safe_to_push_output
                                        );
                                        assert_eq!(
                                            resolved.agent_provider,
                                            explicit_agent_provider
                                                .unwrap_or(AgentProvider::OpenCode)
                                        );
                                        assert_eq!(
                                            resolved.publish_to_registry,
                                            publish_to_registry
                                        );
                                    }
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
