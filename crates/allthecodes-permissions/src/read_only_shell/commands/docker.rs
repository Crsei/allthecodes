use std::collections::HashMap;

use super::super::flag_validation::map_from_pairs;
use super::super::types::{ExternalCommandConfig, FlagArgType};

pub(crate) fn make_docker_read_only_commands() -> HashMap<String, ExternalCommandConfig> {
    let mut m = HashMap::new();

    m.insert(
        "docker ps".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-a", FlagArgType::None),
            ("--all", FlagArgType::None),
            ("-f", FlagArgType::String),
            ("--filter", FlagArgType::String),
            ("--format", FlagArgType::String),
            ("-n", FlagArgType::Number),
            ("--last", FlagArgType::Number),
            ("-l", FlagArgType::None),
            ("--latest", FlagArgType::None),
            ("-q", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("-s", FlagArgType::None),
            ("--size", FlagArgType::None),
            ("--no-trunc", FlagArgType::None),
        ])),
    );

    m.insert(
        "docker images".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-a", FlagArgType::None),
            ("--all", FlagArgType::None),
            ("-f", FlagArgType::String),
            ("--filter", FlagArgType::String),
            ("--format", FlagArgType::String),
            ("-q", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("--no-trunc", FlagArgType::None),
            ("--digests", FlagArgType::None),
            ("--tree", FlagArgType::None),
        ])),
    );

    m.insert(
        "docker inspect".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::String),
            ("--format", FlagArgType::String),
            ("-s", FlagArgType::None),
            ("--size", FlagArgType::None),
            ("--type", FlagArgType::String),
        ])),
    );

    m.insert(
        "docker network ls".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::String),
            ("--filter", FlagArgType::String),
            ("--format", FlagArgType::String),
            ("-q", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("--no-trunc", FlagArgType::None),
        ])),
    );

    m.insert(
        "docker volume ls".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::String),
            ("--filter", FlagArgType::String),
            ("--format", FlagArgType::String),
            ("-q", FlagArgType::None),
            ("--quiet", FlagArgType::None),
            ("--no-trunc", FlagArgType::None),
        ])),
    );

    m.insert(
        "docker logs".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::None),
            ("--follow", FlagArgType::None),
            ("--tail", FlagArgType::String),
            ("-t", FlagArgType::None),
            ("--timestamps", FlagArgType::None),
            ("--details", FlagArgType::None),
            ("-n", FlagArgType::Number),
        ])),
    );

    m.insert(
        "docker info".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::String),
            ("--format", FlagArgType::String),
        ])),
    );

    m.insert(
        "docker version".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-f", FlagArgType::String),
            ("--format", FlagArgType::String),
        ])),
    );

    m.insert(
        "docker stats".into(),
        ExternalCommandConfig::new(map_from_pairs(&[
            ("-a", FlagArgType::None),
            ("--all", FlagArgType::None),
            ("--format", FlagArgType::String),
            ("--no-stream", FlagArgType::None),
            ("--no-trunc", FlagArgType::None),
        ])),
    );

    m
}
