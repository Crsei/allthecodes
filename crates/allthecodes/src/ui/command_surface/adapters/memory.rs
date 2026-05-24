use std::path::{Path, PathBuf};

use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::memory::memory_file_selector::{
    MemoryFileKind, MemoryFileOption, MemoryFileSelectorState,
};
pub(crate) fn selected_memory_open_command(
    state: &MemoryFileSelectorState,
) -> CommandSurfaceOutcome {
    let Some(option) = state.options.get(state.selected_index) else {
        return CommandSurfaceOutcome::None;
    };
    if option.kind != MemoryFileKind::Folder {
        return CommandSurfaceOutcome::Submit("/memory edit".to_string());
    }

    let description = option.description.to_ascii_lowercase();
    let scope = if description.contains("auto") {
        "auto"
    } else if description.contains("team") {
        "team"
    } else if description.contains("global") {
        "global"
    } else {
        "project"
    };
    CommandSurfaceOutcome::Submit(format!("/memory open {scope}"))
}

pub(crate) fn memory_options(cwd: &Path, home: &Path) -> Vec<MemoryFileOption> {
    let mut options = vec![
        file_option(home.join("AGENTS.md"), MemoryFileKind::User),
        file_option(cwd.join("AGENTS.md"), MemoryFileKind::Project),
    ];
    for path in allthecodes_config::claude_md::find_agents_md_files(cwd) {
        let project_agents = cwd.join("AGENTS.md");
        if path != project_agents {
            options.push(file_option(path, MemoryFileKind::Nested));
        }
    }
    options.extend([
        MemoryFileOption::new(
            allthecodes_config::paths::auto_memory_dir(),
            MemoryFileKind::Folder,
        )
        .with_description("auto-memory folder"),
        MemoryFileOption::new(
            allthecodes_config::paths::team_memory_dir(cwd),
            MemoryFileKind::Folder,
        )
        .with_description("team-memory folder"),
        MemoryFileOption::new(
            allthecodes_config::paths::memory_dir_global(),
            MemoryFileKind::Folder,
        )
        .with_description("global memory folder"),
        MemoryFileOption::new(
            cwd.join(".allthecodes").join("memory"),
            MemoryFileKind::Folder,
        )
        .with_description("project memory folder"),
    ]);
    options
}

pub(crate) fn file_option(path: PathBuf, kind: MemoryFileKind) -> MemoryFileOption {
    let exists = path.exists();
    let option = MemoryFileOption::new(path, kind);
    if exists {
        option
    } else {
        option.missing()
    }
}
