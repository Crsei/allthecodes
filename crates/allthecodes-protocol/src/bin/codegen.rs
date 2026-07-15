use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use allthecodes_protocol::codegen::{self, ArtifactTarget};

const USAGE: &str = "Usage:\n  codegen --target backend-docs [--check]\n  codegen --target frontend --frontend-dir <path> [--check]\n  codegen --target all --frontend-dir <path> [--check]\n\nOptions:\n  --target <backend-docs|frontend|all>  Select generated artifact owner(s)\n  --frontend-dir <path>                Explicit frontend generated directory\n  --check                              Check committed bytes without writing\n  -h, --help                           Print this help";

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    target: ArtifactTarget,
    frontend_dir: Option<PathBuf>,
    check: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum ParseResult {
    Run(Cli),
    Help,
}

fn main() -> ExitCode {
    let parsed = match parse_args(std::env::args_os().skip(1)) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match parsed {
        ParseResult::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ParseResult::Run(cli) => match execute(&cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        },
    }
}

fn execute(cli: &Cli) -> Result<(), codegen::CodegenError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let paths = codegen::artifact_paths(manifest_dir, cli.target, cli.frontend_dir.as_deref())?;

    for path in &paths {
        println!("resolved artifact: {}", path.display());
    }

    let completed = if cli.check {
        codegen::check_api_artifacts_up_to_date(
            manifest_dir,
            cli.target,
            cli.frontend_dir.as_deref(),
        )
    } else {
        codegen::write_api_artifacts(manifest_dir, cli.target, cli.frontend_dir.as_deref())
    }?;

    let action = if cli.check { "checked" } else { "wrote" };
    for path in completed {
        println!("{action}: {}", path.display());
    }
    Ok(())
}

fn parse_args<I>(args: I) -> Result<ParseResult, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut target = None;
    let mut frontend_dir = None;
    let mut check = false;
    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        let argument = argument
            .into_string()
            .map_err(|_| "arguments must be valid UTF-8".to_string())?;
        match argument.as_str() {
            "-h" | "--help" => return Ok(ParseResult::Help),
            "--check" => {
                if check {
                    return Err("--check may only be provided once".to_string());
                }
                check = true;
            }
            "--target" => {
                if target.is_some() {
                    return Err("--target may only be provided once".to_string());
                }
                let value = required_value(&mut args, "--target")?;
                target = Some(ArtifactTarget::parse(&value).map_err(|error| error.to_string())?);
            }
            "--frontend-dir" => {
                if frontend_dir.is_some() {
                    return Err("--frontend-dir may only be provided once".to_string());
                }
                frontend_dir = Some(PathBuf::from(required_value(&mut args, "--frontend-dir")?));
            }
            _ if argument.starts_with("--target=") => {
                if target.is_some() {
                    return Err("--target may only be provided once".to_string());
                }
                let value = argument.trim_start_matches("--target=");
                if value.is_empty() {
                    return Err("--target requires a value".to_string());
                }
                target = Some(ArtifactTarget::parse(value).map_err(|error| error.to_string())?);
            }
            _ if argument.starts_with("--frontend-dir=") => {
                if frontend_dir.is_some() {
                    return Err("--frontend-dir may only be provided once".to_string());
                }
                let value = argument.trim_start_matches("--frontend-dir=");
                if value.is_empty() {
                    return Err("--frontend-dir requires a value".to_string());
                }
                frontend_dir = Some(PathBuf::from(value));
            }
            _ => return Err(format!("unknown argument '{argument}'")),
        }
    }

    let target = target.ok_or_else(|| "--target is required".to_string())?;
    Ok(ParseResult::Run(Cli {
        target,
        frontend_dir,
        check,
    }))
}

fn required_value<I>(args: &mut I, option: &str) -> Result<String, String>
where
    I: Iterator<Item = OsString>,
{
    let value = args
        .next()
        .ok_or_else(|| format!("{option} requires a value"))?
        .into_string()
        .map_err(|_| format!("{option} value must be valid UTF-8"))?;
    if value.starts_with('-') {
        return Err(format!("{option} requires a value"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ParseResult, String> {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_each_supported_target() {
        assert_eq!(
            parse(&["--target", "backend-docs", "--check"]),
            Ok(ParseResult::Run(Cli {
                target: ArtifactTarget::BackendDocs,
                frontend_dir: None,
                check: true,
            }))
        );
        assert_eq!(
            parse(&["--target=frontend", "--frontend-dir", "/tmp/generated"]),
            Ok(ParseResult::Run(Cli {
                target: ArtifactTarget::Frontend,
                frontend_dir: Some(PathBuf::from("/tmp/generated")),
                check: false,
            }))
        );
        assert_eq!(
            parse(&["--target", "all", "--frontend-dir=/tmp/generated"]),
            Ok(ParseResult::Run(Cli {
                target: ArtifactTarget::All,
                frontend_dir: Some(PathBuf::from("/tmp/generated")),
                check: false,
            }))
        );
    }

    #[test]
    fn rejects_missing_unknown_and_duplicate_arguments() {
        assert_eq!(parse(&[]), Err("--target is required".to_string()));
        assert!(parse(&["--target", "invalid"]).is_err());
        assert!(parse(&["--wat"]).is_err());
        assert!(parse(&["--target", "backend-docs", "--target", "all"]).is_err());
        assert!(parse(&["--target", "backend-docs", "--check", "--check"]).is_err());
        assert!(parse(&["--target", "frontend", "--frontend-dir"]).is_err());
    }

    #[test]
    fn help_short_circuits_required_target() {
        assert_eq!(parse(&["--help"]), Ok(ParseResult::Help));
        assert_eq!(parse(&["-h"]), Ok(ParseResult::Help));
    }
}
