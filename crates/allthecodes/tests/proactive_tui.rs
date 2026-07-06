#[test]
fn root_cli_dependency_surface_exposes_proactive_command_metadata() {
    let commands = allthecodes_commands::get_all_commands();
    let metadata = allthecodes_commands::command_metadata(&commands);

    let proactive = metadata
        .iter()
        .find(|command| command.name == "proactive")
        .expect("/proactive command metadata");

    assert_eq!(proactive.description, "Toggle proactive autonomous mode");
}
