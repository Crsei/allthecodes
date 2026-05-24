mod docker;
mod gh;
mod git;
mod tools;

use std::collections::HashMap;

use super::types::ExternalCommandConfig;

pub(crate) use docker::make_docker_read_only_commands;
pub(crate) use gh::make_gh_read_only_commands;
pub(crate) use git::make_git_read_only_commands;
pub(crate) use tools::{make_pyright_read_only_commands, make_rg_read_only_commands};

pub(crate) fn make_external_readonly_commands() -> HashMap<String, ExternalCommandConfig> {
    let mut m = HashMap::new();

    for cmd in &[
        "ls",
        "cat",
        "head",
        "tail",
        "echo",
        "printf",
        "which",
        "type",
        "pwd",
        "whoami",
        "id",
        "date",
        "uname",
        "env",
        "printenv",
        "true",
        "false",
        "yes",
        "dirname",
        "basename",
        "readlink",
        "realpath",
        "tty",
        "wc",
        "uniq",
        "cut",
        "tr",
        "fold",
        "nl",
        "od",
        "hexdump",
        "xxd",
        "strings",
        "df",
        "du",
        "stat",
        "lsblk",
        "blkid",
        "lscpu",
        "lsusb",
        "lspci",
        "arch",
        "nproc",
        "getconf",
        "mkvextract",
        "mediainfo",
        "ffprobe",
        "exiftool",
        "file",
        "mimetype",
        "jq",
        "yq",
        "mlr",
        "ps",
        "top",
        "htop",
        "btop",
        "uptime",
        "w",
        "lsof",
        "fuser",
        "ip",
        "ifconfig",
        "ss",
        "netstat",
        "route",
        "arp",
        "bat",
        "batcat",
        "delta",
        "diff",
        "diff3",
        "colordiff",
        "tree",
        "exa",
        "eza",
        "lsd",
        "rg",
        "ripgrep",
        "ag",
        "ack",
        "grep",
        "egrep",
        "fgrep",
        "pt",
        "tldr",
        "man",
        "whatis",
        "apropos",
    ] {
        m.insert(cmd.to_string(), ExternalCommandConfig::new(HashMap::new()));
    }

    m
}
