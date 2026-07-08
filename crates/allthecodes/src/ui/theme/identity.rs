use ratatui::style::{Color, Modifier, Style};

use super::ThemeColors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIdentityRole {
    Primary,
    Planner,
    Executor,
    Reviewer,
    Tester,
    Researcher,
    Background,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentIdentity<'a> {
    pub agent_id: &'a str,
    pub role: Option<&'a str>,
    pub is_primary: bool,
}

pub fn role_from_agent_role(role: Option<&str>, is_primary: bool) -> AgentIdentityRole {
    if is_primary {
        return AgentIdentityRole::Primary;
    }
    let normalized = role.unwrap_or("").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "planner" | "plan" | "architect" => AgentIdentityRole::Planner,
        "executor" | "builder" | "worker" | "implementer" => AgentIdentityRole::Executor,
        "reviewer" | "review" | "critic" => AgentIdentityRole::Reviewer,
        "tester" | "test" | "qa" | "verifier" => AgentIdentityRole::Tester,
        "researcher" | "research" | "explorer" | "investigator" => AgentIdentityRole::Researcher,
        "background" | "daemon" | "monitor" => AgentIdentityRole::Background,
        _ => AgentIdentityRole::Unknown,
    }
}

pub fn agent_identity_color(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Color {
    match role_from_agent_role(identity.role, identity.is_primary) {
        AgentIdentityRole::Primary => colors.accent,
        AgentIdentityRole::Planner => colors.agentPlanner,
        AgentIdentityRole::Executor => colors.agentExecutor,
        AgentIdentityRole::Reviewer => colors.agentReviewer,
        AgentIdentityRole::Tester => colors.agentTester,
        AgentIdentityRole::Researcher => colors.agentResearcher,
        AgentIdentityRole::Background => colors.agentBackground,
        AgentIdentityRole::Unknown => {
            let palette = [
                colors.agentBlue,
                colors.agentCyan,
                colors.agentPurple,
                colors.agentPink,
                colors.agentOrange,
                colors.agentBackground,
            ];
            let index = (fnv1a64(identity.agent_id.as_bytes()) as usize) % palette.len();
            palette[index]
        }
    }
}

pub fn agent_identity_style(colors: &ThemeColors, identity: AgentIdentity<'_>) -> Style {
    Style::default()
        .fg(agent_identity_color(colors, identity))
        .add_modifier(Modifier::BOLD)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{get_theme, ThemeName};
    use ratatui::style::{Color, Modifier};

    fn dark() -> &'static ThemeColors {
        get_theme(&ThemeName::Dark)
    }

    #[test]
    fn primary_agent_keeps_assistant_purple() {
        let style = agent_identity_style(
            dark(),
            AgentIdentity {
                agent_id: "primary",
                role: Some("main"),
                is_primary: true,
            },
        );
        assert_eq!(style.fg, Some(Color::Rgb(190, 140, 255)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn known_roles_use_reserved_safe_identity_colours() {
        let planner = agent_identity_color(
            dark(),
            AgentIdentity {
                agent_id: "agent-planner",
                role: Some("planner"),
                is_primary: false,
            },
        );
        let executor = agent_identity_color(
            dark(),
            AgentIdentity {
                agent_id: "agent-executor",
                role: Some("executor"),
                is_primary: false,
            },
        );
        let reviewer = agent_identity_color(
            dark(),
            AgentIdentity {
                agent_id: "agent-reviewer",
                role: Some("reviewer"),
                is_primary: false,
            },
        );

        assert_eq!(planner, dark().agentPlanner);
        assert_eq!(executor, dark().agentExecutor);
        assert_eq!(reviewer, dark().agentReviewer);
        assert_ne!(reviewer, dark().warning);
        assert_ne!(executor, dark().success);
    }

    #[test]
    fn unknown_agent_hash_is_stable() {
        let first = agent_identity_color(
            dark(),
            AgentIdentity {
                agent_id: "worker-thread-123",
                role: Some("unknown"),
                is_primary: false,
            },
        );
        let second = agent_identity_color(
            dark(),
            AgentIdentity {
                agent_id: "worker-thread-123",
                role: None,
                is_primary: false,
            },
        );
        assert_eq!(first, second);
    }
}
